use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_dir() -> std::path::PathBuf {
    static ID: AtomicU64 = AtomicU64::new(1);
    let path = std::env::temp_dir().join(format!(
        "neoflash-xfl-replace-{}-{}",
        std::process::id(),
        ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("create XFL replacement test directory");
    path
}

#[test]
fn long_xfl_contour_continues_across_bounded_edges() {
    let mut commands = vec![PathCommand::MoveTo { x: 1.0, y: 2.0 }];
    for x in 2..15_000 {
        commands.push(PathCommand::LineTo {
            x: x as f64,
            y: 3.0,
        });
    }
    let edges = path_commands_to_xfl_edges(&commands, true);
    assert!(edges.len() > 1);
    assert!(edges.iter().all(|edge| edge.len() <= 65_535));
    for adjacent in edges.windows(2) {
        let last = adjacent[0].rsplit('|').next().unwrap();
        assert!(adjacent[1].starts_with(&format!("!{last}|")));
    }
    assert!(edges.last().unwrap().ends_with("|20 40"));
}

#[test]
fn staged_xfl_install_failure_restores_previous_target_and_cleans_swap_paths() {
    let root = temp_dir();
    let target = root.join("movie.xfl");
    let staged = root.join(".movie.xfl-tmp");
    let backup = root.join(".movie.xfl-backup");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("marker.txt"), b"previous document").unwrap();
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join("marker.txt"), b"new document").unwrap();

    let mut rename_count = 0usize;
    let error = replace_staged_path_recoverably_with(&staged, &target, &backup, |from, to| {
        rename_count += 1;
        if rename_count == 2 {
            Err(std::io::Error::other("injected staged install failure"))
        } else {
            fs::rename(from, to)
        }
    })
    .expect_err("second rename is injected to fail");

    assert!(
        error
            .to_string()
            .contains("injected staged install failure")
    );
    assert_eq!(rename_count, 3, "target move, failed install, rollback");
    assert_eq!(
        fs::read(target.join("marker.txt")).unwrap(),
        b"previous document"
    );
    assert!(!staged.exists(), "staged directory must be cleaned");
    assert!(!backup.exists(), "rollback must consume backup directory");

    fs::remove_dir_all(root).unwrap();
}
