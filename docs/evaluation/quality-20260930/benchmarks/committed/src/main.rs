use orbit_research_store::corpus::Corpus;
use serde_json::json;
use std::{path::Path, time::Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let corpus = Corpus::open(Path::new(&args[1])).expect("open benchmark corpus");
    let count: usize = args[2].parse().expect("record count");
    let samples: usize = args[3].parse().expect("sample count");
    assert!(samples > 0 && samples % 2 == 1, "use an odd sample count");
    let expected = corpus.snapshot().expect("expected working snapshot");

    for committed in [false, true] {
        let mut durations = Vec::with_capacity(samples);
        for sample in 0..=samples {
            let start = Instant::now();
            let snapshot = if committed {
                corpus.committed_snapshot()
            } else {
                corpus.snapshot()
            }
            .expect("read public snapshot API");
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(snapshot.revision, expected.revision);
            assert_eq!(snapshot.records.len(), count);
            for (index, record) in snapshot.records.iter().enumerate() {
                assert_eq!(record.id, format!("Q{:03}", index + 1));
                assert_eq!(record.body, format!("Benchmark body {}.\n", index + 1));
                assert_eq!(record.git_blob, expected.records[index].git_blob);
                assert_eq!(
                    record.content_sha256,
                    expected.records[index].content_sha256
                );
            }
            if sample > 0 {
                durations.push(elapsed_ms);
            }
        }
        let mut sorted = durations.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "{}",
            json!({
                "api": if committed { "committed_snapshot" } else { "snapshot" },
                "records": count,
                "warmups": 1,
                "samples_ms": durations,
                "median_ms": sorted[samples / 2],
                "fixture_commit": expected.revision,
            })
        );
    }
}
