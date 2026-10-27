//! Timing on a large file: `ARMLS_PERF=big.s cargo test -p arm-lsp --release perf -- --nocapture`.

use std::time::Instant;

#[test]
fn perf() {
    let Ok(path) = std::env::var("ARMLS_PERF") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read");
    let time = |label: &str, f: &mut dyn FnMut()| {
        let runs = 5;
        let start = Instant::now();
        for _ in 0..runs {
            f();
        }
        eprintln!("{label:>24}: {:?}", start.elapsed() / runs);
    };
    time("parse", &mut || drop(arm_syntax::parse(&text)));
    time("analysis", &mut || drop(arm_sema::Analysis::new(&text)));
    let config = arm_lint::Config::default();
    time("check (sema + lint)", &mut || {
        drop(arm_lint::check(
            &text,
            None,
            &config,
            Default::default(),
            true,
        ))
    });
    let analysis = arm_sema::Analysis::new(&text);
    time("call graph", &mut || drop(arm_lint::call_graph(&analysis)));
    time("semantic tokens", &mut || {
        drop(crate::ide::semantic_tokens(&analysis))
    });
    time("inlay hints", &mut || {
        drop(crate::ide::inlay_hints(&analysis))
    });
    time("folding", &mut || {
        drop(crate::ide::folding_ranges(&analysis))
    });
    time("outline", &mut || drop(crate::ide::outline(&analysis)));
    let end = text.len() - 30;
    time("completion", &mut || {
        drop(crate::ide::completions(&analysis, end, &[]))
    });
    time("signature help", &mut || {
        drop(crate::ide::signature_help(&analysis, end))
    });
    time("hover", &mut || drop(crate::ide::hover(&analysis, end)));
    time("references", &mut || {
        drop(crate::ide::references(&analysis, end, true))
    });
    time("format", &mut || {
        drop(arm_syntax::format(
            &text,
            &arm_syntax::FormatOptions::default(),
        ))
    });
    let mut index = crate::workspace::Index::default();
    time("index update", &mut || {
        index.update(std::path::Path::new("/x/big.s"), &text)
    });
}

#[test]
fn perf_loop() {
    let Ok(path) = std::env::var("ARMLS_PERF_LOOP") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read");
    let config = arm_lint::Config::default();
    for _ in 0..40 {
        let checked = arm_lint::check(&text, None, &config, Default::default(), true);
        drop(crate::ide::semantic_tokens(&checked.analysis));
    }
}

#[test]
fn perf_scan() {
    let Ok(dir) = std::env::var("ARMLS_PERF_SCAN") else {
        return;
    };
    let start = Instant::now();
    let index = crate::workspace::Index::scan(&[std::path::PathBuf::from(dir)]);
    eprintln!(
        "scan of {} files: {:?}",
        index.files().len(),
        start.elapsed()
    );
}
