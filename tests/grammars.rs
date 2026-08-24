use yara::core::syntax::Syntax;

fn colored(ext: &str, code: &str) -> usize {
    let syntax = Syntax::default();
    let mut distinct = std::collections::HashSet::new();
    syntax.highlight_lines(ext, code, |regions| {
        for r in regions {
            if !r.text.trim().is_empty() {
                distinct.insert(r.color);
            }
        }
    });
    distinct.len()
}

#[test]
fn bundled_and_aliased_languages_highlight() {
    let cases: &[(&str, &str)] = &[
        (
            "ts",
            "export const x: number = 1; // note\nfunction f(a: string) { return `hi ${a}`; }",
        ),
        ("tsx", "const A = () => <div/>; // jsx"),
        ("toml", "[package]\nname = \"yara\" # comment\nversion = 2"),
        ("kt", "fun main() { val s: String = \"hi\" } // note"),
        (
            "swift",
            "func f() -> Int { let s = \"hi\"; return 1 } // note",
        ),
        ("dart", "class A { void f() { var s = 'hi'; } } // note"),
        ("Dockerfile", "FROM rust:1 AS build\nRUN cargo build # note"),
        (
            "proto",
            "syntax = \"proto3\";\nmessage M { int32 id = 1; } // note",
        ),
        ("graphql", "type Query { user(id: ID!): User } # note"),
        // Aliased to a close relative rather than falling back to plain text.
        ("mjs", "export const a = 1; // note"),
        ("ex", "def f do\n  :ok # note\nend"),
        ("zig", "const std = @import(\"std\"); // note"),
        ("ini", "[section]\nkey = value ; note"),
    ];
    for (ext, code) in cases {
        let n = colored(ext, code);
        assert!(
            n > 1,
            "{ext} produced {n} distinct colors — no grammar matched"
        );
    }
}

#[test]
fn language_names_resolve() {
    let syntax = Syntax::default();
    assert_eq!(syntax.language_name("ts"), "TypeScript");
    assert_eq!(syntax.language_name("toml"), "TOML");
    assert_eq!(syntax.language_name("rs"), "Rust");
    assert_eq!(syntax.language_name("mjs"), "JavaScript");
    assert_eq!(syntax.language_name("wat"), "Plain Text");
}
