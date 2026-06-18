//! End-to-end discovery against a fixture project tree (no device, no Gradle).

use androkit::project;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn discovers_multi_variant_project() {
    let root = fixture("multi-variant");
    let project = project::discover_uncached(&root).expect("discovery should succeed");

    // Modules: :app (application) and :core (library).
    assert_eq!(project.modules.len(), 2);
    assert_eq!(project.app_module.as_deref(), Some(":app"));
    let app = project.modules.iter().find(|m| m.path == ":app").unwrap();
    assert!(app.is_application);
    let core = project.modules.iter().find(|m| m.path == ":core").unwrap();
    assert!(!core.is_application);

    // applicationId.
    assert_eq!(
        project.application_id.as_deref(),
        Some("com.example.sample")
    );

    // Variants: {dev,prod} × {debug,release} = 4.
    let names: Vec<&str> = project.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names.len(), 4);
    for expected in ["devDebug", "devRelease", "prodDebug", "prodRelease"] {
        assert!(names.contains(&expected), "missing variant {expected}");
    }

    // Default variant prefers devDebug.
    assert_eq!(project.default_variant.as_deref(), Some("devDebug"));

    // Launcher activity resolved & fully-qualified as a component.
    assert_eq!(
        project.launch_activity.as_deref(),
        Some("com.example.sample/com.example.sample.ui.MainActivity")
    );
}

#[test]
fn derives_task_names_from_convention() {
    let root = fixture("multi-variant");
    let project = project::discover_uncached(&root).unwrap();
    assert_eq!(project.install_task("devDebug"), "installDevDebug");
    assert_eq!(project.unit_test_task("devDebug"), "testDevDebugUnitTest");
    assert_eq!(project.assemble_task("prodRelease"), "assembleProdRelease");
}

#[test]
fn find_root_walks_up_from_nested_dir() {
    let nested = fixture("multi-variant").join("app/src/main");
    let root = project::find_root(&nested).expect("should find root");
    assert_eq!(root, fixture("multi-variant"));
}
