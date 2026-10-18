// @lfy def/cli/main.lfy:main
//! A change set's copy of the root and its base: what is copied into them, which of their
//! files are the change set's own, what a resumed copy takes from the root, what the change
//! set changed, and how the root takes every change set at once.

use super::*;

/// Where elfie-compile keeps the change set of each target: its file, and beside it a folder
/// named after the target holding its copy and its base.
// @lfy def/cli/main.lfy:main#main:main:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
pub(crate) const CHANGES: &str = "elfie-compile/cache/changes";

/// What a copy of the root leaves behind: the cache the copies live in, and the folder the
/// CLI writes its own files to under the root.
// @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
// @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
pub(crate) const UNCOPIED: [&str; 2] = ["elfie-compile/cache", "elfie-requests"];

/// The directories a listing of files skips wherever they are: build products, version
/// control, and installed packages.
// @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
// @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
pub(crate) const SKIPPED: [&str; 3] = ["target", ".git", "node_modules"];

/// Whether a path is under one of these folders, which are spelled relative to the root with
/// forward slashes as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
pub(crate) fn under(path: &str, folders: &[&str]) -> bool {
    folders.iter().any(|folder| path == *folder || path.starts_with(&format!("{folder}/")))
}

/// Whether a path, relative to the root, passes through a directory named target, .git, or
/// node_modules.
// @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
pub(crate) fn in_skipped(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    segments[..segments.len().saturating_sub(1)].iter().any(|segment| SKIPPED.contains(segment))
}

/// The one file that tells git in a copy of the root which repository it is looking at, named
/// as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:8a305990c26ec5a21e609fca17bc010a769c49511853e849d7b5b29db5b1b906
pub(crate) const GITFILE: &str = ".git";

/// Whether a file is executable, as the mode the platform keeps it in; nothing where the
/// platform has no such mode, and nothing for a file that cannot be read.
// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
#[cfg(unix)]
pub(crate) fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).ok().map(|data| data.permissions().mode())
}

// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
#[cfg(not(unix))]
pub(crate) fn mode_of(_path: &Path) -> Option<u32> {
    None
}

/// A file left in the mode its source was in, so a script a change set copied or merged can
/// still be run.
// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt as _;
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
#[cfg(not(unix))]
pub(crate) fn set_mode(_path: &Path, _mode: Option<u32>) {}

/// One file copied to another path, byte for byte, keeping whether it is executable, the
/// folders it goes into made when they are missing.
// @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
pub(crate) fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let held = fs::read(from)?;
    // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    // @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
    fs::write(to, held)?;
    set_mode(to, mode_of(from));
    Ok(())
}

/// The copy of a target's change set: `elfie-compile/cache/changes/<target>/copy` under the
/// root, as an absolute path when the root is one.
// @lfy def/cli/main.lfy:main#main:main:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
pub(crate) fn copy_of(root: &Path, target: &str) -> PathBuf {
    root.join(CHANGES).join(target).join("copy")
}

/// The base of a target's change set: `elfie-compile/cache/changes/<target>/base` under the
/// root.
// @lfy def/cli/main.lfy:main#main:main:13ac3d0d600577ad94fbbc002962bb3f615a9df8b9c329232562285f53032386
pub(crate) fn base_of(root: &Path, target: &str) -> PathBuf {
    root.join(CHANGES).join(target).join("base")
}

/// Every file a new copy of the root holds, by its path relative to the root with forward
/// slashes, in path order: every file git tracks or would track, as `git ls-files --cached
/// --others --exclude-standard` lists them in a root that is a git repository, and otherwise
/// every file of the root found recursively outside every directory named target, .git, or
/// node_modules; never one under elfie-compile/cache or elfie-requests.
// @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
// @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
pub(crate) fn listing(root: &Path) -> Vec<String> {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    match git_directory(root) {
        // @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
        Some(_) => {
            let listed = git(root, &["ls-files", "--cached", "--others", "--exclude-standard"]).unwrap_or_default();
            for path in listed.lines().map(str::trim).filter(|path| !path.is_empty()) {
                // A file git lists that is not there, because it was removed since, is
                // nothing to copy.
                if root.join(path).is_file() {
                    paths.insert(path.to_string());
                }
            }
        }
        // @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
        None => {
            let mut found = Vec::new();
            walk_listed(root, root, &mut found);
            paths.extend(found.iter().map(|path| relative(root, path)));
        }
    }
    // @lfy def/cli/main.lfy:main#main:main:670dca2fbcb84c029ce58dc80e12e107f388904bf0cf79378ec5b4136e8b6097
    // @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
    paths.into_iter().filter(|path| !under(path, &UNCOPIED) && path != GITFILE).collect()
}

/// Every file under a directory of the root, skipping every directory named target, .git,
/// or node_modules and never going into elfie-compile/cache or elfie-requests, where the
/// copies themselves are.
// @lfy def/cli/main.lfy:main#main:main:7209ec1dd07cd609c4d0c6340e068b2f8abc452938bf80a2546b4339a185641e
fn walk_listed(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let named = relative(root, &path);
            if path.file_name().is_some_and(|name| SKIPPED.iter().any(|skipped| name == *skipped))
                || under(&named, &UNCOPIED)
            {
                continue;
            }
            walk_listed(root, &path, out);
        } else {
            out.push(path);
        }
    }
}

/// Whether a path, relative to a copy, is one of the change set's own: under the target's
/// output directory outside every directory named target, .git, or node_modules, or under
/// `elfie-compile/maps/<target>` or `elfie-compile/reviews/<target>`. The copy's `.git` file,
/// the cache, and the CLI's own folder are never among them.
// @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
pub(crate) fn is_own(path: &str, target: &Target) -> bool {
    if path == GITFILE || under(path, &UNCOPIED) {
        return false;
    }
    // @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
    let maps = format!("{MAPS}/{}", target.identifier);
    let reviews = format!("{REVIEWS}/{}", target.identifier);
    if under(path, &[maps.as_str(), reviews.as_str()]) {
        return true;
    }
    // @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
    crate::processes::within(path, &target.output_directory) && !in_skipped(path)
}

/// Every one of a change set's own files that a directory holds, by its path relative to it,
/// in path order.
// @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
pub(crate) fn own_files(directory: &Path, target: &Target) -> Vec<String> {
    let mut found = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:f49e520eb6aa39ddc79bfc4e49e3543abb0374d3e5f23e3668a8a68de4bc2616
    for folder in [
        target.output_directory.clone(),
        format!("{MAPS}/{}", target.identifier),
        format!("{REVIEWS}/{}", target.identifier),
    ] {
        walk_all(&directory.join(&folder), &mut found);
    }
    let mut paths: Vec<String> =
        found.iter().map(|path| relative(directory, path)).filter(|path| is_own(path, target)).collect();
    paths.sort();
    paths.dedup();
    paths
}

/// A directory removed with everything in it; nothing there is no error.
// @lfy def/cli/main.lfy:main#main:main:c40cbc04ab7381898fa0276bff3403d5869070d1ce78a5380f696993bf5a34dd
pub(crate) fn remove_directory(directory: &Path) -> io::Result<()> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// A new copy of the root for a target's change set, and its base, made under the root;
/// gives back the copy's absolute path.
///
/// A directory at the path of the copy or of the base is removed first. The copy holds every
/// file [`listing`] gives, copied to the same path byte for byte, keeping whether it is
/// executable, and a `.git` file reading `gitdir:`, a space, and the absolute path of the
/// root's git directory when the root is a git repository, so that git run in the copy reads
/// the root's history. The base holds, at the same paths, every one of the change set's own
/// files as the copy held it when it was made.
// @lfy def/cli/main.lfy:main#main:main:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
// @lfy def/cli/main.lfy:main#main:main:c40cbc04ab7381898fa0276bff3403d5869070d1ce78a5380f696993bf5a34dd
// @lfy def/cli/main.lfy:main#main:main:13ac3d0d600577ad94fbbc002962bb3f615a9df8b9c329232562285f53032386
// @lfy def/cli/main.lfy:main#main:main:8a305990c26ec5a21e609fca17bc010a769c49511853e849d7b5b29db5b1b906
pub(crate) fn make_copy(root: &Path, target: &Target) -> io::Result<PathBuf> {
    let copy = copy_of(root, &target.identifier);
    let base = base_of(root, &target.identifier);
    // @lfy def/cli/main.lfy:main#main:main:c40cbc04ab7381898fa0276bff3403d5869070d1ce78a5380f696993bf5a34dd
    remove_directory(&copy)?;
    remove_directory(&base)?;
    fs::create_dir_all(&copy)?;
    // @lfy def/cli/main.lfy:main#main:main:8b9a6852b850dca4a74c1925af07379b2968a5bef89b107388fd48072d032118
    for path in listing(root) {
        copy_file(&root.join(&path), &copy.join(&path))?;
    }
    // @lfy def/cli/main.lfy:main#main:main:8a305990c26ec5a21e609fca17bc010a769c49511853e849d7b5b29db5b1b906
    if let Some(at) = git_directory(root) {
        fs::write(copy.join(GITFILE), format!("gitdir: {}\n", at.to_string_lossy()))?;
    }
    // @lfy def/cli/main.lfy:main#main:main:13ac3d0d600577ad94fbbc002962bb3f615a9df8b9c329232562285f53032386
    fs::create_dir_all(&base)?;
    for path in own_files(&copy, target) {
        copy_file(&copy.join(&path), &base.join(&path))?;
    }
    // @lfy def/cli/main.lfy:main#main:main:46046c83c1cfa40ed79495fde1ef0777d6db2402a03b330c3064d14c34546303
    Ok(std::path::absolute(&copy).unwrap_or(copy))
}

/// A kept copy brought up to the root before its first unit runs: every file a new copy
/// would hold that is none of the change set's own files is copied into it from the root,
/// and every file of the copy that is none of its own, is not its `.git` file, is not one a
/// new copy would hold, and is under no directory named target, .git, or node_modules, is
/// removed. Its own files, its base, and what a build left under those directories stay as
/// they are.
// @lfy def/cli/main.lfy:main#main:main:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
pub(crate) fn sync_copy(root: &Path, copy: &Path, target: &Target) -> io::Result<()> {
    let wanted: BTreeSet<String> = listing(root).into_iter().collect();
    // @lfy def/cli/main.lfy:main#main:main:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
    for path in &wanted {
        if is_own(path, target) {
            continue;
        }
        let from = root.join(path);
        let to = copy.join(path);
        if fs::read(&from).ok() != fs::read(&to).ok() || mode_of(&from) != mode_of(&to) {
            copy_file(&from, &to)?;
        }
    }
    // The removal walks no directory named target, .git, or node_modules.
    // @lfy def/cli/main.lfy:main#main:main:489059c8a497ac06d146f6905d881772dce1f6e60f48adf6f5bd5b6ca509bdc2
    let mut found = Vec::new();
    walk_listed(copy, copy, &mut found);
    for path in found {
        let named = relative(copy, &path);
        if named == GITFILE || under(&named, &UNCOPIED) || is_own(&named, target) || wanted.contains(&named) {
            continue;
        }
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// What one of a change set's own files holds in its copy and what its base held: one of the
/// change set's changes.
// @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Change {
    /// What the base held for it; nothing when the change set added it.
    // @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
    pub(crate) base: Option<Vec<u8>>,
    /// What the copy holds for it; nothing when the change set removed it.
    // @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
    pub(crate) text: Option<Vec<u8>>,
    /// Whether the copy's file is executable, so the file merged keeps it.
    // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
    pub(crate) mode: Option<u32>,
}

/// The changes of one change set, by path, in path order.
// @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
pub(crate) type Changes = BTreeMap<String, Change>;

/// Every one of a change set's own files that differs from its base, was added, or was
/// removed, with what the base held for it and what the copy holds, in path order.
///
/// What is compared is every path the base holds and every own file the copy holds; in a
/// root that is a git repository a file the change set added counts only when git would
/// track it, as `git ls-files --others --exclude-standard` lists it in the copy. A path of the
/// base counts as removed only when nothing is at that path in the copy.
// @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
// @lfy def/cli/main.lfy:main#main:main:76e8b3b4bed342c6f79c5f24800d5a700d7eccaad184dc55edcd275cd5a9e32d
// @lfy def/cli/main.lfy:main#main:main:09d98aeb33f1262c0723d2f55d871762843d90677cc202153ba4a0ad4792759d
pub(crate) fn changes_of(root: &Path, copy: &Path, target: &Target) -> Changes {
    let base = base_of(root, &target.identifier);
    let held: BTreeSet<String> = own_files(&base, target).into_iter().collect();
    // A file the change set added counts only when git would track it.
    // @lfy def/cli/main.lfy:main#main:main:76e8b3b4bed342c6f79c5f24800d5a700d7eccaad184dc55edcd275cd5a9e32d
    let tracked: Option<BTreeSet<String>> = git_directory(root).map(|_| {
        git(copy, &["ls-files", "--others", "--exclude-standard"])
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim().to_string())
            .collect()
    });
    let mut paths: BTreeSet<String> = held.clone();
    for path in own_files(copy, target) {
        // @lfy def/cli/main.lfy:main#main:main:76e8b3b4bed342c6f79c5f24800d5a700d7eccaad184dc55edcd275cd5a9e32d
        if held.contains(&path) || tracked.as_ref().is_none_or(|tracked| tracked.contains(&path)) {
            paths.insert(path);
        }
    }
    let mut changes: Changes = BTreeMap::new();
    for path in paths {
        let at = copy.join(&path);
        let text = fs::read(&at).ok();
        // A path counts as removed only when nothing is at it in the copy; one that is there
        // and cannot be read is left out, so the root's file stands rather than going.
        // @lfy def/cli/main.lfy:main#main:main:09d98aeb33f1262c0723d2f55d871762843d90677cc202153ba4a0ad4792759d
        if text.is_none() && at.exists() {
            continue;
        }
        let was = fs::read(base.join(&path)).ok();
        // @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
        if was == text {
            continue;
        }
        changes.insert(path, Change { base: was, text, mode: mode_of(&at) });
    }
    changes
}

/// What merging changes into a directory writes: each path with what it becomes, nothing
/// where it is removed, and the mode it keeps.
// @lfy def/cli/main.lfy:main#main:main:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
pub(crate) type Writes = Vec<(String, Option<Vec<u8>>, Option<u32>)>;

/// What every change becomes once merged into a directory, file by file, written nowhere:
/// where the directory's file still holds what the base holds it becomes the change's, or is
/// removed when the change removed it, and where something changed it since the base was
/// taken, it holds the changes of both, as the three-way merge of the two against the base.
/// The one file that cannot be merged without a conflict is what it gives back instead.
// @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
// @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
pub(crate) fn merges_of(changes: &Changes, into: &Path, scratch: &Path) -> Result<Writes, String> {
    let mut writes: Writes = Vec::new();
    for (path, change) in changes {
        let theirs = fs::read(into.join(path)).ok();
        // The file still holds what the base holds: it becomes the change's text, or is
        // removed when the change removed it.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        if theirs.as_deref() == change.base.as_deref() {
            writes.push((path.clone(), change.text.clone(), change.mode));
            continue;
        }
        // It already holds what the change holds: there is nothing to merge.
        if theirs.as_deref() == change.text.as_deref() {
            continue;
        }
        // Something else changed the file since the base was taken: it holds the changes of
        // both, as the three-way merge of it and the change's text against the base. One of
        // the three missing is a file added or removed on both sides, which no merge settles.
        // @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
        let (Some(base), Some(mine), Some(theirs)) =
            (change.base.as_deref(), change.text.as_deref(), theirs.as_deref())
        else {
            // @lfy def/cli/main.lfy:main#main:main:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
            return Err(path.clone());
        };
        // @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
        match merged(theirs, base, mine, scratch) {
            Some(text) => writes.push((path.clone(), Some(text), change.mode)),
            // @lfy def/cli/main.lfy:main#main:main:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
            None => return Err(path.clone()),
        }
    }
    Ok(writes)
}

/// What a merge worked out written into a directory: each file becomes what it was merged
/// into, keeping the mode the change's file is in, or is removed; the paths written.
// @lfy def/cli/main.lfy:main#main:main:7faf028d9e62dd4a82a75683cb6aef188e05dc77f9dbd53210dd7cba92ba7277
// @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
pub(crate) fn write_merges(writes: &Writes, into: &Path) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    for (path, text, mode) in writes {
        let to = into.join(path);
        match text {
            Some(text) => {
                // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::write(&to, text).is_err() {
                    return Err(path.clone());
                }
                // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
                set_mode(&to, *mode);
            }
            // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
            None => {
                let _ = fs::remove_file(&to);
            }
        }
        written.push(path.clone());
    }
    Ok(written)
}

/// The changes of every change set merged into the root at once: nothing of the root changes
/// until every file of every change set has merged without a conflict; the one file that
/// could not be merged otherwise, and nothing of the root changed.
// @lfy def/cli/main.lfy:main#main:main:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
// @lfy def/cli/main.lfy:main#main:main:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
pub(crate) fn merge_all(every: &[Changes], root: &Path) -> Result<Vec<String>, String> {
    let scratch = root.join(CHANGES).join("merge");
    let mut writes: Writes = Vec::new();
    // @lfy def/cli/main.lfy:main#main:main:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
    for changes in every {
        writes.extend(merges_of(changes, root, &scratch)?);
    }
    // @lfy def/cli/main.lfy:main#main:main:f62308cbdc193558b2d780fd52bf6661146cd7a2e58d88bb29026d348fb53412
    write_merges(&writes, root)
}

/// The three-way merge of two sides against their base, as `git merge-file` gives it;
/// nothing when it conflicts.
// @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
pub(crate) fn merged(theirs: &[u8], base: &[u8], mine: &[u8], scratch: &Path) -> Option<Vec<u8>> {
    // @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
    fs::create_dir_all(scratch).ok()?;
    let sides = [("theirs", theirs), ("base", base), ("mine", mine)];
    for (name, held) in sides {
        fs::write(scratch.join(name), held).ok()?;
    }
    // @lfy def/cli/main.lfy:main#main:main:eade10ea32d1fd58426767f451ba8db4ba721952a1d1d8f7f61428fa7c1b7eb0
    let output = Process::new("git")
        .args(["merge-file", "-p", "--quiet", "theirs", "base", "mine"])
        .current_dir(scratch)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let _ = fs::remove_dir_all(scratch);
    // A code of 0 is a clean merge; anything above it counts the conflicts, and anything
    // below it is git itself failing.
    // @lfy def/cli/main.lfy:main#main:main:7ea6410a8673a0a0b1c2459596d167fac7037f12858c4c99845e67852a0efb8b
    (output.status.code() == Some(0)).then_some(output.stdout)
}

/// A change set gone: its file under elfie-compile/cache, its copy, and its base, as when the
/// root has taken it or nothing of it is to be kept.
// @lfy def/cli/main.lfy:main#main:main:5d1e63160121bb44aeb91bd7d1f6eced00a1991447dd7d14e77ff2e4a4bf2f9a
pub(crate) fn remove_change_set(root: &Path, set: &ChangeSet) -> io::Result<()> {
    // @lfy def/cli/main.lfy:main#main:main:5d1e63160121bb44aeb91bd7d1f6eced00a1991447dd7d14e77ff2e4a4bf2f9a
    set.remove(root)?;
    remove_directory(&copy_of(root, &set.target))?;
    remove_directory(&base_of(root, &set.target))?;
    // The folder that held them goes too, once nothing is left in it.
    let _ = fs::remove_dir(root.join(CHANGES).join(&set.target));
    Ok(())
}
