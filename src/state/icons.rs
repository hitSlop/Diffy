//! App-owned image copies. Call from a worker, never during rendering.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Copy an image beside the preferences, using its contents to avoid name
/// collisions. Publish only a complete, synced copy; leave the source untouched.
pub fn import_project_icon(state_path: &Path, source: &Path) -> Result<PathBuf, String> {
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| gpui_kit::Img::extensions().contains(&e.as_str()))
        .ok_or("Choose an image such as PNG, JPEG, WebP, or SVG.")?;
    let parent = state_path.parent().ok_or("Image storage is unavailable.")?;
    let directory = parent.join("project-icons");
    let mut source = fs::File::open(source).map_err(|e| format!("Couldn't read image: {e}"))?;
    if !source
        .metadata()
        .map_err(|e| format!("Couldn't read image: {e}"))?
        .is_file()
    {
        return Err("Choose an image file.".into());
    }
    fs::create_dir_all(&directory).map_err(|e| format!("Couldn't save image: {e}"))?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = directory.join(format!(".image-{}-{suffix}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<PathBuf> {
        let mut copy = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
            copy.write_all(&buffer[..count])?;
        }
        copy.sync_all()?;
        drop(copy);
        let destination = directory.join(format!("{:x}.{extension}", hash.finalize()));
        fs::rename(&temporary, &destination)?;
        Ok(destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Couldn't save image: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_images_survive_source_changes_and_avoid_filename_collisions() {
        let root = std::env::temp_dir().join(format!("diffy-icon-copy-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let state = root.join("app/state.json");
        let source = root.join("icon.PNG");
        fs::write(&source, b"first image").unwrap();
        let first = import_project_icon(&state, &source).unwrap();
        assert_eq!(
            first.parent(),
            Some(root.join("app/project-icons").as_path())
        );
        assert_eq!(first.extension().unwrap(), "png");
        assert_eq!(fs::read(&source).unwrap(), b"first image");
        assert_eq!(import_project_icon(&state, &source).unwrap(), first);
        // Selecting an already-owned image is also safe.
        assert_eq!(import_project_icon(&state, &first).unwrap(), first);
        fs::write(&source, b"second image").unwrap();
        let second = import_project_icon(&state, &source).unwrap();
        assert_ne!(first, second);
        fs::remove_file(&source).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"first image");
        assert_eq!(fs::read(&second).unwrap(), b"second image");
        assert!(import_project_icon(&state, &source).is_err());
        assert_eq!(fs::read(&first).unwrap(), b"first image");
        assert_eq!(fs::read_dir(first.parent().unwrap()).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_imports_leave_existing_files_untouched() {
        let root = std::env::temp_dir().join(format!("diffy-icon-failure-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let state = root.join("state.json");
        let source = root.join("icon.svg");
        fs::write(&source, b"<svg/>").unwrap();
        fs::write(root.join("project-icons"), b"blocked directory").unwrap();
        assert!(import_project_icon(&state, &source).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"<svg/>");
        assert_eq!(
            fs::read(root.join("project-icons")).unwrap(),
            b"blocked directory"
        );
        let invalid = root.join("script.sh");
        fs::write(&invalid, b"echo hello").unwrap();
        assert!(import_project_icon(&state, &invalid).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
