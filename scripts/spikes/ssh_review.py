#!/usr/bin/env python3
"""Read-only OpenSSH review study. No app integration or remote helper installation.

python3 scripts/spikes/ssh_review.py --self-test --report target/ssh-study.json
python3 scripts/spikes/ssh_review.py my-host /absolute/repository --preview src/main.rs
Requires trusted known_hosts and key/agent authentication. Never prompts for secrets.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shlex
import socket
import statistics
import subprocess
import tempfile
import time


class ReviewError(RuntimeError):
    pass


class Connection:
    def __init__(self, host, root, config=None, timeout=30):
        if not host or host.startswith('-') or any(c.isspace() or c == '\0' for c in host):
            raise ValueError('Use an SSH host alias without spaces or a leading dash.')
        if not root.startswith('/') or '\0' in root:
            raise ValueError('Repository path must be absolute.')
        self.host, self.root, self.timeout = host, root, timeout
        self.temp = tempfile.TemporaryDirectory(prefix='diffy-ssh-', dir='/tmp')
        self.control = str(Path(self.temp.name) / 'socket')
        self.options = ['ssh', '-T', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                        '-o', 'ConnectTimeout=10', '-o', 'ServerAliveInterval=5',
                        '-o', 'ServerAliveCountMax=2', '-o', 'ControlMaster=auto',
                        '-o', 'ControlPersist=30', '-o', 'ControlPath=' + self.control]
        if config:
            self.options += ['-F', str(config)]

    def command(self, args):
        # OpenSSH sends a command string to the remote shell. Quote every argument
        # for the supported POSIX shells, including repository paths and filenames.
        remote = ' '.join(shlex.quote(str(arg)) for arg in args)
        try:
            result = subprocess.run(self.options + ['--', self.host, remote],
                                    capture_output=True, timeout=self.timeout)
        except subprocess.TimeoutExpired as error:
            raise ReviewError('Remote read timed out; the local SSH request was cancelled.') from error
        if result.returncode:
            raise ReviewError(result.stderr.decode('utf-8', errors='replace').strip() or 'Remote read failed.')
        return result.stdout

    def git(self, args):
        return self.command(['git', '--no-pager', '--no-optional-locks', '--literal-pathspecs',
                             '-c', 'diff.autoRefreshIndex=false', '-c', 'core.fsmonitor=false',
                             '-C', self.root] + args)

    def refresh(self):
        status = self.git(['status', '--porcelain=v2', '-z', '--untracked-files=all'])
        trees = self.git(['worktree', 'list', '--porcelain', '-z'])
        return {'files': status_rows(status), 'worktrees': worktree_rows(trees)}

    def read_file(self, path):
        relative_path(path)
        script = ('cd "$1" || exit; '
                  'if test -L "./$2"; then printf "L\\000"; readlink -n "./$2"; '
                  'elif test -f "./$2"; then printf "F\\000"; dd if="./$2" bs=1024 count=4097 2>/dev/null; '
                  'else echo "Selected file is unavailable" >&2; exit 1; fi')
        raw = self.command(['sh', '-c', script, 'diffy-study', self.root, path])
        kind, content = raw.split(b'\0', 1)
        if kind not in (b'L', b'F'):
            raise ReviewError('Unexpected remote file response.')
        if len(content) > 4 * 1024 * 1024:
            raise ReviewError('Selected untracked/conflicted file exceeds the 4 MiB study limit.')
        if b'\0' in content:
            raise ReviewError('Selected file is binary.')
        return content.decode('utf-8')

    def preview(self, path, staged=False):
        relative_path(path)
        rows = status_rows(self.git(['status', '--porcelain=v2', '-z', '--untracked-files=all']))
        entry = next((row for row in rows if row['path'] == path), None)
        if not entry:
            raise ReviewError('Selected file has no changes.')
        if entry['kind'] in ('untracked', 'conflicted'):
            return {'kind': entry['kind'], 'source': self.read_file(path)}
        args = ['diff', '--no-color', '--no-ext-diff', '--no-textconv', '--find-renames',
                '--src-prefix=a/', '--dst-prefix=b/', '--unified=1000000']
        if staged:
            args.append('--cached')
        args += ['--'] + ([entry['original']] if entry.get('original') else []) + [path]
        return {'kind': 'patch', 'patch': self.git(args).decode('utf-8')}

    def close(self):
        try:
            subprocess.run(self.options + ['-O', 'exit', '--', self.host], capture_output=True, timeout=10)
        except subprocess.TimeoutExpired:
            pass
        finally:
            self.temp.cleanup()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def relative_path(path):
    if not path or '\0' in path or PurePosixPath(path).is_absolute() or '..' in PurePosixPath(path).parts:
        raise ValueError('Selected path must stay inside the repository.')


def status_rows(raw):
    records = iter(raw.decode('utf-8').split('\0'))
    rows = []
    for record in records:
        if not record:
            continue
        kind = record[0]
        if kind == '?':
            rows.append({'kind': 'untracked', 'path': record[2:], 'staged': False, 'changes': True})
        elif kind in ('1', '2', 'u'):
            fields = record.split(' ', {'1': 8, '2': 9, 'u': 10}[kind])
            xy = fields[1]
            row = {'kind': 'conflicted' if kind == 'u' else 'tracked', 'path': fields[-1],
                   'staged': xy[0] != '.', 'changes': xy[1] != '.'}
            if kind == '2':
                row['original'] = next(records)
            rows.append(row)
    return rows


def worktree_rows(raw):
    rows, current = [], {}
    for field in raw.decode('utf-8').split('\0'):
        if not field:
            if current:
                rows.append(current)
                current = {}
            continue
        key, _, value = field.partition(' ')
        current[key] = value if value else True
    return rows


def checked(args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, **kwargs).stdout


def self_test():
    """Disposable localhost sshd, keys, known_hosts, and repositories only."""
    with tempfile.TemporaryDirectory(prefix='diffy-ssh-fixture-', dir='/tmp') as tmp:
        temp = Path(tmp)
        for key in ('host', 'client', 'rejected'):
            checked(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(temp / key)])
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        sshd_config = temp / 'sshd_config'
        sshd_config.write_text(f'''Port {port}
ListenAddress 127.0.0.1
HostKey {temp / 'host'}
AuthorizedKeysFile {temp / 'client.pub'}
PidFile {temp / 'pid'}
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
LogLevel ERROR
''')
        pub = (temp / 'host.pub').read_text().split()
        (temp / 'known_hosts').write_text(f'[127.0.0.1]:{port} {pub[0]} {pub[1]}\n')
        config = temp / 'ssh_config'
        config.write_text(f'''Host fixture
  HostName 127.0.0.1
  Port {port}
  User {os.environ.get('USER', 'jordan')}
  IdentityFile {temp / 'client'}
  IdentitiesOnly yes
  UserKnownHostsFile {temp / 'known_hosts'}
''')
        log = open(temp / 'sshd.log', 'wb')
        server = subprocess.Popen(['/usr/sbin/sshd', '-D', '-e', '-f', str(sshd_config)], stdout=log, stderr=log)
        try:
            ready = False
            for _ in range(100):
                if server.poll() is not None:
                    break
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=.1):
                        ready = True
                        break
                except OSError:
                    time.sleep(.02)
            if not ready:
                raise ReviewError('Local fixture sshd unavailable: ' + (temp / 'sshd.log').read_text())
            root = temp / "repo with quote' and\nnewline"
            root.mkdir()
            def git(*args, check=True):
                return subprocess.run(['git', '-C', str(root), '-c', 'user.name=Study',
                    '-c', 'user.email=study@example.invalid', '-c', 'commit.gpgsign=false',
                    '-c', 'core.hooksPath=/dev/null', *args], check=check, capture_output=True).stdout
            git('init', '--initial-branch=main')
            (root / 'partial.txt').write_text('one\ntwo\n')
            (root / 'conflict.txt').write_text('base\n')
            git('add', '.')
            git('commit', '-m', 'initial')
            git('checkout', '-b', 'other')
            (root / 'conflict.txt').write_text('other\n')
            git('commit', '-am', 'other')
            git('checkout', 'main')
            (root / 'conflict.txt').write_text('main\n')
            git('commit', '-am', 'main')
            git('merge', 'other', check=False)
            (root / 'partial.txt').write_text('staged\ntwo\n')
            git('add', 'partial.txt')
            (root / 'partial.txt').write_text('staged\nworking\n')
            literal = "new 'ü\nfile.txt"
            (root / literal).write_text('untracked\n')
            (root / ';touch INJECTED').write_text('literal\n')
            (root / 'symlink').symlink_to('target with\nnewline\n')
            linked = temp / 'linked'
            git('worktree', 'add', '-b', 'linked', str(linked), 'HEAD')
            before = (root / '.git/index').read_bytes()
            stamp = (root / '.git/index').stat().st_mtime_ns
            with Connection('fixture', str(root), config) as connection:
                start = time.perf_counter()
                snapshot = connection.refresh()
                cold = (time.perf_counter() - start) * 1000
                assert snapshot['files'] == status_rows(git('--no-optional-locks', 'status', '--porcelain=v2', '-z', '--untracked-files=all'))
                assert len(snapshot['worktrees']) == 2
                assert connection.preview(literal)['source'] == 'untracked\n'
                assert connection.preview(';touch INJECTED')['source'] == 'literal\n'
                assert connection.preview('symlink')['source'] == 'target with\nnewline\n'
                assert not (root / 'INJECTED').exists()
                assert 'staged' in connection.preview('partial.txt', staged=True)['patch']
                assert 'working' in connection.preview('partial.txt')['patch']
                assert '<<<<<<<' in connection.preview('conflict.txt')['source']
                warm, previews = [], []
                for _ in range(30):
                    start = time.perf_counter(); connection.refresh(); warm.append((time.perf_counter() - start) * 1000)
                    start = time.perf_counter(); connection.preview('partial.txt'); previews.append((time.perf_counter() - start) * 1000)
                def fails(read):
                    try: read()
                    except (ReviewError, ValueError): return True
                    raise AssertionError('Expected a recoverable error')
                missing_path = fails(lambda: connection.preview('gone'))
                with Connection('fixture', str(root / 'missing'), config) as missing:
                    missing_repo = fails(missing.refresh)
                missing_git = fails(lambda: connection.command(['diffy-study-missing-git']))
                rejected_config = temp / 'rejected_config'
                rejected_config.write_text(config.read_text().replace(str(temp / 'client'), str(temp / 'rejected')))
                with Connection('fixture', str(root), rejected_config) as rejected:
                    authentication = fails(rejected.refresh)
                wrong_hosts = temp / 'wrong_hosts'
                wrong_key = (temp / 'rejected.pub').read_text().split()
                wrong_hosts.write_text(f'[127.0.0.1]:{port} {wrong_key[0]} {wrong_key[1]}\n')
                wrong_config = temp / 'wrong_config'
                wrong_config.write_text(config.read_text().replace(str(temp / 'known_hosts'), str(wrong_hosts)))
                with Connection('fixture', str(root), wrong_config) as untrusted:
                    host_verification = fails(untrusted.refresh)
                connection.timeout = .1
                cancellation = fails(lambda: connection.command(['sleep', '2']))
                connection.timeout = 30
                assert before == (root / '.git/index').read_bytes()
                assert stamp == (root / '.git/index').stat().st_mtime_ns
            # A fresh connection cannot survive a stopped server.
            server.terminate(); server.wait(timeout=5)
            with Connection('fixture', str(root), config, timeout=2) as offline:
                disconnected = fails(offline.refresh)
            report = {'transport': 'system-openssh', 'remote_helper': False, 'read_only': True,
                'index_bytes_and_mtime_preserved': True, 'staged_unstaged_untracked_conflicted': True,
                'literal_paths_and_shell_arguments': True, 'worktrees': 2,
                'missing_path': missing_path, 'missing_repository': missing_repo,
                'missing_git': missing_git, 'timeout_cancellation': cancellation, 'disconnect': disconnected,
                'authentication_rejected': authentication, 'host_key_mismatch_rejected': host_verification,
                'cold_refresh_ms': cold, 'iterations': 30,
                'warm_refresh': {'median_ms': statistics.median(warm), 'p95_ms': sorted(warm)[28]},
                'selected_preview': {'median_ms': statistics.median(previews), 'p95_ms': sorted(previews)[28]},
                'index_sha256': hashlib.sha256(before).hexdigest(),
                'limits': ['localhost fixture, not WAN measurements', 'POSIX shells only',
                    'trusted known_hosts and non-interactive key/agent auth required',
                    '4 MiB cap on untracked/conflicted source reads', 'production UI and polling policy deferred']}
            return report
        finally:
            if server.poll() is None:
                server.terminate(); server.wait(timeout=5)
            log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('host', nargs='?')
    parser.add_argument('root', nargs='?')
    parser.add_argument('--config')
    parser.add_argument('--preview')
    parser.add_argument('--staged', action='store_true')
    parser.add_argument('--self-test', action='store_true')
    parser.add_argument('--report')
    args = parser.parse_args()
    if args.self_test:
        report = self_test()
    else:
        if not args.host or not args.root:
            parser.error('Supply an SSH host and absolute repository path, or --self-test.')
        with Connection(args.host, args.root, args.config) as connection:
            report = connection.preview(args.preview, args.staged) if args.preview else connection.refresh()
    text = json.dumps(report, indent=2, ensure_ascii=False)
    if args.report:
        Path(args.report).write_text(text + '\n')
    print(text)


if __name__ == '__main__':
    main()
