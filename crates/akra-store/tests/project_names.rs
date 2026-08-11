use std::path::Path;

use akra_store::{ProjectName, ProjectNameError, ProjectNames};

#[test]
fn suggests_the_workspace_basename() {
    let name = ProjectName::suggest_from_path(Path::new(r"C:\dev\akra-hookers"));

    assert_eq!(name.display(), "akra-hookers");
    assert_eq!(name.normalized(), "akra-hookers");
}

#[test]
fn suggests_the_git_root_name_or_final_path_component() {
    assert_eq!(
        ProjectName::suggest_from_git_root_or_path(
            Some(Path::new("/work/root-project")),
            Path::new("/work/root-project/crates/store"),
        )
        .display(),
        "root-project"
    );
    assert_eq!(
        ProjectName::suggest_from_git_root_or_path(None, Path::new("/work/fallback-project"))
            .display(),
        "fallback-project"
    );
}

#[test]
fn preserves_korean_display_spelling_while_normalizing_for_identity() {
    let name = ProjectName::parse("  아크라 프로젝트  ").unwrap();

    assert_eq!(name.display(), "아크라 프로젝트");
    assert_eq!(name.normalized(), "아크라 프로젝트");
}

#[test]
fn trims_outer_unicode_whitespace_but_preserves_display_spelling() {
    let name = ProjectName::parse("\u{2003}  MiXeD Name  \u{00a0}").unwrap();

    assert_eq!(name.display(), "MiXeD Name");
    assert_eq!(name.normalized(), "mixed name");
}

#[test]
fn normalizes_nfkc_full_width_latin_and_lowercases_without_locale() {
    let full_width = ProjectName::parse("ＡＫＲＡ").unwrap();
    let mixed_case = ProjectName::parse("aKrA").unwrap();

    assert_eq!(full_width.normalized(), "akra");
    assert_eq!(full_width.normalized(), mixed_case.normalized());
}

#[test]
fn collapses_repeated_unicode_whitespace_to_one_ascii_space_in_normalized_value() {
    let name = ProjectName::parse("Akra\u{2003}\u{00a0}\tHookers").unwrap();

    assert_eq!(name.display(), "Akra\u{2003}\u{00a0}\tHookers");
    assert_eq!(name.normalized(), "akra hookers");
}

#[test]
fn rejects_controls_and_blank_names_with_typed_errors() {
    assert!(matches!(
        ProjectName::parse("akra\u{0007}"),
        Err(ProjectNameError::ControlCharacter { .. })
    ));
    assert!(matches!(
        ProjectName::parse("\u{2003}\t\u{00a0}"),
        Err(ProjectNameError::Blank)
    ));
}

#[test]
fn accepts_eighty_unicode_scalars_and_rejects_eighty_one() {
    let eighty = "가".repeat(80);
    let eighty_one = "가".repeat(81);

    let accepted = ProjectName::parse(&eighty).unwrap();
    assert_eq!(accepted.display(), eighty);
    assert!(matches!(
        ProjectName::parse(&eighty_one),
        Err(ProjectNameError::TooLong {
            maximum: 80,
            actual: 81
        })
    ));
}

#[test]
fn allocates_the_smallest_available_suffix_from_normalized_collisions() {
    let mut names = ProjectNames::new();

    assert_eq!(names.allocate("origin-a", "Name").display(), "Name");
    assert_eq!(
        names.allocate("origin-b", "ＮＡＭＥ").display(),
        "ＮＡＭＥ (2)"
    );
    assert_eq!(names.allocate("origin-c", "name").display(), "name (3)");
}

#[test]
fn distinct_origins_with_the_same_basename_remain_distinct_projects() {
    let mut names = ProjectNames::new();

    let first = names.allocate(r"C:\one\api", "api");
    let second = names.allocate(r"D:\two\api", "api");

    assert_ne!(first.origin(), second.origin());
    assert_eq!(first.display(), "api");
    assert_eq!(second.display(), "api (2)");
}

#[test]
fn allocating_the_same_origin_is_idempotent() {
    let mut names = ProjectNames::new();

    let first = names.allocate("origin-a", "Initial Name");
    let repeated = names.allocate("origin-a", "Ignored Rename");

    assert_eq!(repeated, first);
    assert_eq!(repeated.display(), "Initial Name");
    assert_eq!(repeated.origin(), Some("origin-a"));
}

#[test]
fn suffix_allocation_skips_occupied_names_and_fills_the_smallest_gap() {
    let mut names = ProjectNames::new();

    assert_eq!(
        names.allocate("preoccupied", "Name (2)").display(),
        "Name (2)"
    );
    assert_eq!(names.allocate("base", "Name").display(), "Name");
    assert_eq!(
        names.allocate("collision", "ＮＡＭＥ").display(),
        "ＮＡＭＥ (3)"
    );

    let mut gap = ProjectNames::new();
    assert_eq!(gap.allocate("base", "Name").display(), "Name");
    assert_eq!(gap.allocate("third", "Name (3)").display(), "Name (3)");
    assert_eq!(gap.allocate("collision", "name").display(), "name (2)");
}

#[test]
fn basename_less_paths_use_a_valid_deterministic_fallback() {
    let first = ProjectName::suggest_from_path(Path::new("/"));
    let second = ProjectName::suggest_from_path(Path::new("/"));

    assert_eq!(first, second);
    assert_eq!(first.display(), "Untitled Project");
    assert_eq!(first.normalized(), "untitled project");
}
