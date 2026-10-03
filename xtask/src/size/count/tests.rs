use super::{ModDecl, analyze, merged_len};
use crate::size::fixtures::{code, inline_tests};

fn counted(source: &str) -> usize {
    analyze(source).unwrap().counted
}

#[test]
fn plain_file_counts_every_line() {
    assert_eq!(counted(&code(12)), 12);
    assert_eq!(counted(""), 0);
}

#[test]
fn blank_lines_and_comments_count() {
    assert_eq!(counted("// one\n\n/// three\npub fn four() {}\n"), 4);
}

#[test]
fn inline_test_module_is_excluded() {
    let source = format!("{}{}", code(300), inline_tests(500));
    assert_eq!(source.lines().count(), 800);
    assert_eq!(counted(&source), 300);
}

#[test]
fn out_of_line_test_module_is_excluded_and_recorded() {
    let source = format!("{}#[cfg(test)]\nmod tests;\nmod real;\n", code(5));
    let analysis = analyze(&source).unwrap();
    assert_eq!(analysis.counted, 6);
    assert_eq!(
        analysis.modules,
        [
            ModDecl {
                parents: vec![],
                name: "tests".into(),
                path: None,
                test_only: true
            },
            ModDecl {
                parents: vec![],
                name: "real".into(),
                path: None,
                test_only: false
            },
        ]
    );
}

#[test]
fn nested_test_items_are_excluded() {
    let source = "\
pub mod outer {
    pub fn real() {}

    /// Docs belong to the item.
    #[cfg(test)]
    fn helper() -> u32 {
        1
    }

    pub struct S;

    impl S {
        #[cfg(test)]
        fn only_in_tests(&self) {}

        pub fn kept(&self) {}
    }

    pub trait T {
        #[cfg(test)]
        fn probe(&self) {}
    }
}
";
    assert_eq!(counted(source), 23 - 5 - 2 - 2);
}

#[test]
fn declarations_inside_test_code_are_test_only() {
    let source = "\
mod a {
    #[path = \"x.rs\"]
    mod b;
}
#[cfg(test)]
mod tests {
    mod helpers;
}
";
    let analysis = analyze(source).unwrap();
    assert_eq!(analysis.counted, 4);
    assert_eq!(
        analysis.modules,
        [
            ModDecl {
                parents: vec!["a".into()],
                name: "b".into(),
                path: Some("x.rs".into()),
                test_only: false
            },
            ModDecl {
                parents: vec!["tests".into()],
                name: "helpers".into(),
                path: None,
                test_only: true
            },
        ]
    );
}

#[test]
fn cfg_predicates() {
    let item = |cfg: &str| format!("#[cfg({cfg})]\nfn f() {{}}\n");
    assert_eq!(counted(&item("test")), 0);
    assert_eq!(counted(&item("all(unix, test)")), 0);
    assert_eq!(counted(&item("all(unix, all(test))")), 0);
    assert_eq!(counted(&item("not(test)")), 2);
    assert_eq!(counted(&item("any(test, unix)")), 2);
    assert_eq!(counted(&item("feature = \"test\"")), 2);
    assert_eq!(counted("#[cfg_attr(test, derive(Debug))]\nstruct S;\n"), 2);
}

#[test]
fn file_level_cfg_test_counts_nothing() {
    let source = format!("#![cfg(test)]\nmod child;\n{}", code(450));
    let analysis = analyze(&source).unwrap();
    assert_eq!(analysis.counted, 0);
    assert!(analysis.modules[0].test_only);
}

#[test]
fn invalid_source_is_an_error() {
    assert!(analyze("fn {").is_err());
}

#[test]
fn merged_len_counts_overlaps_once() {
    assert_eq!(merged_len(&mut []), 0);
    assert_eq!(merged_len(&mut [(3, 5)]), 3);
    assert_eq!(merged_len(&mut [(10, 12), (1, 2), (2, 4), (11, 11)]), 7);
}
