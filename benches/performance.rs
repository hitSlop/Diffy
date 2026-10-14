mod support {
    pub mod fixture;
    pub mod gix;
}
use criterion::Criterion;
use diffy::repository::{self, ChangeGroup, MetadataBackend, PreviewCache, PreviewKey};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use support::fixture::Fixture;

type BenchCase<'a> = (&'static str, Box<dyn FnMut() + 'a>);

fn measure(mut run: impl FnMut()) -> (f64, f64) {
    run();
    let mut values = (0..30)
        .map(|_| {
            let start = Instant::now();
            run();
            start.elapsed().as_secs_f64() * 1000.
        })
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    ((values[14] + values[15]) / 2., values[28])
}
fn main() {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2))
        .configure_from_args();
    let report = std::env::var_os("DIFFY_BENCH_REPORT").is_some();
    let only = std::env::var("DIFFY_BENCH_ONLY").ok();
    for mixed in [false, true] {
        let fixture = Fixture::workload(mixed);
        let root = &fixture.0;
        let label = if mixed { "mixed" } else { "untracked" };
        let listing =
            repository::load_metadata_with_backend(root, None, MetadataBackend::Git).unwrap();
        let root = &listing.root;
        let file = listing.groups[1]
            .iter()
            .find(|f| f.path.as_str() == "large.txt")
            .unwrap();
        let small = listing.groups[1]
            .iter()
            .find(|f| f.path.as_str() != "large.txt")
            .unwrap();
        let key = PreviewKey {
            root: root.clone(),
            group: ChangeGroup::Changes,
            path: small.path.clone(),
            revision: small.revision,
        };
        let mut cache = PreviewCache::default();
        cache.insert(
            key.clone(),
            repository::load_preview(root, ChangeGroup::Changes, small).unwrap(),
        );
        let cases: [BenchCase<'_>; 6] = [
            (
                "eager",
                Box::new(|| {
                    black_box(repository::load(root).unwrap());
                }),
            ),
            (
                "metadata",
                Box::new(|| {
                    black_box(
                        repository::load_metadata_with_backend(
                            root,
                            Some(&listing.context),
                            MetadataBackend::Git,
                        )
                        .unwrap(),
                    );
                }),
            ),
            (
                "selected_preview",
                Box::new(|| {
                    black_box(repository::load_preview(root, ChangeGroup::Changes, file).unwrap());
                }),
            ),
            (
                "cached_navigation",
                Box::new(|| {
                    black_box(cache.get(&key).unwrap());
                }),
            ),
            (
                "gix_metadata",
                Box::new(|| {
                    let snapshot = repository::load_metadata_with_backend(
                        root,
                        Some(&listing.context),
                        MetadataBackend::Gix,
                    )
                    .unwrap();
                    assert_eq!(
                        snapshot.backend,
                        MetadataBackend::Gix,
                        "benchmark must exercise native adapter: {:?}",
                        snapshot.fallback_reason
                    );
                    black_box(snapshot);
                }),
            ),
            (
                "gix_status_prototype",
                Box::new(|| {
                    black_box(support::gix::scope_paths(root).unwrap());
                }),
            ),
        ];
        for (name, mut run) in cases {
            if only.as_ref().is_some_and(|only| only != name) {
                continue;
            }
            if report {
                let (median, p95) = measure(&mut run);
                println!("{label}/{name}: median={median:.6}ms p95={p95:.6}ms");
            } else {
                criterion.bench_function(&format!("{label}/{name}"), |b| b.iter(&mut run));
            }
        }
    }
    if !report {
        criterion.final_summary();
    }
}
