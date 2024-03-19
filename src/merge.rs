use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use glob::glob;
use log::{debug, info, warn};
use regex::Regex;

use crate::ToAnyhow;

/// Regex pattern to find the branch/tag pointed to from the manifest.
const MANIFEST_BRANCH_PATTERN: &str = r"m\/\S* -> (\S*)";

/// Name of subdirectory where merged repository content has to be moved temporarily.
const TMP_TARGET_PATH: &str = "z_tmp_unique_target_directory_@@@";

/// Merge all repositories below `merge_root` into a adjacent git repository with the given suffix.
pub fn merge_repositories(
    merge_root: &str,
    joined_suffix: &str,
    branch: Option<&str>,
) -> Result<()> {
    let target_path = format!("{}{}", merge_root, joined_suffix);
    info!("Repositories below {merge_root} will be merge to {target_path}");

    let repos = find_repos(merge_root).with_context(|| "failed to find repositories")?;
    info!("Found {} repositories to merge", repos.len());

    create_joined_repo(&target_path).with_context(|| "failed to create target repository")?;

    merge_repos(repos.into_iter(), &target_path, merge_root, branch)
        .with_context(|| "failed to merge repositories")
}

fn find_repos(root: &str) -> Result<Vec<PathBuf>> {
    let paths = glob(&format!("{root}/**/.git"))?;

    let mut paths: Vec<_> = paths
        .into_iter()
        .filter_map(|res| {
            res.ok()
                .and_then(|path| path.parent().map(|path| path.to_owned()))
        })
        .collect();

    // Make merge order deterministic.
    paths.sort();

    Ok(paths)
}

fn create_joined_repo(target_path: &str) -> Result<()> {
    fs::create_dir(target_path)?;

    Command::new("git")
        .current_dir(target_path)
        .args(["init"])
        .output()
        .to_anyhow()
        .map(drop)
}

fn merge_repos(
    repos_to_join: impl Iterator<Item = PathBuf>,
    target_path: &str,
    root: &str,
    branch: Option<&str>,
) -> Result<()> {
    let mut exclude = HashSet::from([".git".to_owned(), TMP_TARGET_PATH.to_owned()]);

    let mut prepare_branch = match branch {
        Some(branch) => {
            let branch = branch.to_owned();
            Box::new(move |repo_path: &PathBuf| prepare_requested_branch(repo_path, &branch))
                as Box<dyn FnMut(&PathBuf) -> Result<String>>
        }
        None => {
            let mut manifest_re = Regex::new(MANIFEST_BRANCH_PATTERN).unwrap();
            Box::new(move |repo_path: &PathBuf| {
                prepare_manifest_branch(repo_path, &mut manifest_re)
            }) as Box<dyn FnMut(&PathBuf) -> Result<String>>
        }
    };

    for repo_path in repos_to_join {
        let repo_name = repo_path.strip_prefix(root)?.to_str().unwrap();
        debug!("Merging repo {repo_name}");

        let merge_branch = prepare_branch(&repo_path)?;
        debug!("Using merge branch {merge_branch} in source repository");

        Command::new("git")
            .current_dir(target_path)
            .args(["remote", "add", repo_name, repo_path.to_str().unwrap()])
            .output()
            .to_anyhow()?;

        Command::new("git")
            .current_dir(target_path)
            .args(["fetch", repo_name])
            .output()
            .to_anyhow()?;

        Command::new("git")
            .current_dir(target_path)
            .args([
                "merge",
                &format!("{repo_name}/{merge_branch}"),
                "--allow-unrelated-histories",
            ])
            .output()
            .to_anyhow()?;

        move_repo_contents(&exclude, repo_name, target_path)?;

        // Exclude the merged repository from moves in subsequent merges.
        exclude.insert(repo_name.split('/').next().unwrap().to_owned());

        info!(
            "Merged repository {repo_name} ({})",
            repo_path.to_string_lossy()
        );
    }

    Ok(())
}

fn move_repo_contents(
    exclude: &HashSet<String>,
    repo_name: &str,
    joined_repo_path: &str,
) -> Result<()> {
    // Some repositories contain a folder with their own name, e.g. `googletest/googletest`.
    // To be able to handle them, we move repository content first to a temporary path
    // and then to the permanent location.
    let tmp_repo_target_path = format!("{joined_repo_path}/{TMP_TARGET_PATH}");
    let repo_target_path = format!("{joined_repo_path}/{repo_name}");

    fs::create_dir_all(tmp_repo_target_path)
        .with_context(|| "failed to create temporary repo target path")?;

    let mut top_level_files: Vec<_> = fs::read_dir(joined_repo_path)
        .unwrap()
        .filter_map(|x| {
            x.ok().and_then(|x| {
                let name = x.file_name().to_string_lossy().to_string();
                if exclude.contains(&name) {
                    None
                } else {
                    Some(name)
                }
            })
        })
        .collect();
    top_level_files.sort();

    debug!("Files to move to {repo_name}:");
    for file_ in top_level_files.iter() {
        debug!("  {file_}");
    }

    // Move all merged repository content to temporary path in the joined repository.
    Command::new("git")
        .current_dir(joined_repo_path)
        .args(
            ["mv".to_owned()]
                .into_iter()
                .chain(top_level_files.into_iter())
                .chain([format!("{TMP_TARGET_PATH}/")]),
        )
        .output()
        .to_anyhow()?;

    Command::new("git")
        .current_dir(joined_repo_path)
        .args(["commit", "-m", &format!("Move {repo_name} repo contents")])
        .output()
        .to_anyhow()?;

    // Move all merged repository content to final location.
    // We do this only after merging the content to a temporary path because some content may
    // have the same name as the final location.
    if repo_name.contains('/') {
        // Create parent before moving
        let parent = Path::new(&repo_target_path)
            .parent()
            .with_context(|| "failed to find parent of repo target path")?;
        fs::create_dir_all(parent)?;
    }

    Command::new("git")
        .current_dir(joined_repo_path)
        .args(["mv", &format!("{TMP_TARGET_PATH}"), &format!("{repo_name}")])
        .output()
        .to_anyhow()?;

    Command::new("git")
        .current_dir(joined_repo_path)
        .args(["commit", "--amend", "--no-edit"])
        .output()
        .to_anyhow()?;

    Ok(())
}

fn prepare_manifest_branch(repo_path: &PathBuf, re: &mut Regex) -> Result<String> {
    // Retrieve remote branches in the source repository.
    let output = Command::new("git")
        .current_dir(repo_path)
        .args(["branch", "-r"])
        .output()
        .to_anyhow()?;
    let remote_branches = String::from_utf8_lossy(&output.stdout);

    // Find the branch/tag pointed to by the manifest.
    if let Some(caps) = re.captures(&remote_branches) {
        let manifest_branch = &caps[1];

        if manifest_branch.contains('/') {
            // Regular branch - check it out to have it available for the merge.
            let manifest_branch = manifest_branch
                .split('/')
                .last()
                .map(ToOwned::to_owned)
                .with_context(|| "failed to identify manifest branch")?;

            Command::new("git")
                .current_dir(repo_path)
                .args(["checkout", &manifest_branch])
                .output()
                .to_anyhow()?;

            return Ok(manifest_branch);
        } else {
            // The manifest points to a tag - check it out to a temporary branch name.
            const TMP_JOIN_BRANCH: &str = "tmp_join_branch";

            let res = Command::new("git")
                .current_dir(repo_path)
                .args(["checkout", "-b", TMP_JOIN_BRANCH, &manifest_branch])
                .output()
                .to_anyhow();

            if res.is_err() {
                warn!("Join branch created from tag already exists, continuing...");
            }

            return Ok(TMP_JOIN_BRANCH.to_owned());
        }
    }

    bail!("failed to find manifest branch")
}

fn prepare_requested_branch(repo_path: &PathBuf, branch: &str) -> Result<String> {
    Command::new("git")
        .current_dir(repo_path)
        .args(["checkout", branch])
        .output()
        .to_anyhow()?;

    Ok(branch.to_owned())
}
