// @lfy def/cli/main.lfy:main
//! A batch's own copy of the root: what is copied into it, what it changed, how its files are
//! merged back, and the work a rejected batch keeps for its next run.

use super::*;

/// Where elfie-compile keeps a batch's own copy of the root.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
pub(crate) const BATCHES: &str = "elfie-compile/cache/batches";

/// What a copy of the root leaves behind: the cache the copies live in, and the folder the
/// CLI writes its own files to under the root.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
pub(crate) const UNCOPIED: [&str; 2] = ["elfie-compile/cache", "elfie-requests"];

/// What a merge leaves alone: elfie-compile, whose maps are recorded in the root once the
/// merge is done, elfie-requests, whose files are written there all along, and the copy's own
/// `.git` file, which is the copy's and never the root's.
// @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
pub(crate) const UNMERGED: [&str; 3] = ["elfie-compile", "elfie-requests", GITFILE];

/// Whether a path is under one of these folders, which are spelled relative to the root with
/// forward slashes as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
pub(crate) fn under(path: &str, folders: &[&str]) -> bool {
    folders.iter().any(|folder| path == *folder || path.starts_with(&format!("{folder}/")))
}

/// The one file that tells git in a copy of the root which repository it is looking at, named
/// as the copy spells its own paths.
// @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
pub(crate) const GITFILE: &str = ".git";

/// Whether a file is executable, as the mode the platform keeps it in; nothing where the
/// platform has no such mode, and nothing for a file that cannot be read.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(unix)]
pub(crate) fn mode_of(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).ok().map(|data| data.permissions().mode())
}

// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(not(unix))]
pub(crate) fn mode_of(_path: &Path) -> Option<u32> {
    None
}

/// A file left in the mode its source was in, so a script a batch copied or merged can still
/// be run.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt as _;
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
#[cfg(not(unix))]
pub(crate) fn set_mode(_path: &Path, _mode: Option<u32>) {}

/// One batch's own copy of the root: the directory its compiler and its verifier run in, and
/// what each copied file held when the copy was taken, which is the batch's base.
///
/// A copy exists because compilers editing one tree break each other's builds and tests. A
/// batch's work reaches the root only once its outputs are accepted and reviewed, merged file
/// by file, so the root only ever holds work that passed and the root holds none of a batch's
/// changes while it is not done.
// @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
// @lfy def/cli/main.lfy:main#main:main:241b41c6ff82ebbd898b2d723f95b5c8d5928b57aa500ba825c546edeaccc6a1
pub(crate) struct Mirror {
    pub(crate) directory: PathBuf,
    /// What each copied file held, by its path relative to the root.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    pub(crate) base: BTreeMap<String, Vec<u8>>,
}

impl Mirror {
    /// Every file git lists in the root, except those under the cache and elfie-requests,
    /// copied byte for byte to the same path under `elfie-compile/cache/batches/<batch>`.
    /// Nothing is copied when the root is no git repository, since there is then no list of
    /// what belongs to the project.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    pub(crate) fn take(root: &Path, identifier: &str) -> Option<Mirror> {
        let git_directory = git_directory(root)?;
        // Every file of the root that git tracks or would track, as git ls-files --cached
        // --others --exclude-standard lists them.
        // @lfy def/cli/main.lfy:main#main:main:36bc370dd66b7ca611fe08e5c76eaae494bbb41af5a20c0a009308069c1ac3c5
        let listed = git(root, &["ls-files", "--cached", "--others", "--exclude-standard"])?;
        let directory = root.join(BATCHES).join(file_name_of(identifier));
        // A directory left at the copy's path by an earlier compile is removed before the
        // copy is made, so that nothing of it is taken for the batch's own work.
        // @lfy def/cli/main.lfy:main#main:main:168f11962e12b9d11505b3c6db25a6b72cc753f958945a34524ffce7ab7c2d43
        let _ = fs::remove_dir_all(&directory);
        let mut base = BTreeMap::new();
        for path in listed.lines() {
            let path = path.trim();
            // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
            if path.is_empty() || under(path, &UNCOPIED) {
                continue;
            }
            // A file git lists that is not there, because it was removed since, is nothing
            // to copy.
            // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
            let from = root.join(path);
            let Ok(held) = fs::read(&from) else { continue };
            let to = directory.join(path);
            // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
            if let Some(parent) = to.parent()
                && fs::create_dir_all(parent).is_err()
            {
                return None;
            }
            // Each file is copied to the same path in the copy, byte for byte.
            // @lfy def/cli/main.lfy:main#main:main:6d5d0a607c1fba3bb9c583e43d16675dc224689f89716341f1f894f0ca7f0586
            if fs::write(&to, &held).is_err() {
                return None;
            }
            // The copy keeps whether the file is executable, so a script the compiler is run
            // through can still be started in the copy.
            // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
            // @lfy def/cli/main.lfy:main#main:main:6d5d0a607c1fba3bb9c583e43d16675dc224689f89716341f1f894f0ca7f0586
            set_mode(&to, mode_of(&from));
            // What each copied file held when the batch started is kept as the batch's base.
            // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
            base.insert(path.to_string(), held);
        }
        // The copy is pointed at the root's own repository by a `.git` file, so that git run
        // in it reads the root's history and its index, and nothing of git has to be put in
        // the environment of anything the batch runs.
        // @lfy def/cli/main.lfy:main#main:main:12899ead30416b53072d96627b1e63022facd0087666a2a48961725a3a6d7fb1
        if fs::create_dir_all(&directory).is_err()
            || fs::write(
                directory.join(GITFILE),
                format!("gitdir: {}\n", git_directory.to_string_lossy()),
            )
            .is_err()
        {
            return None;
        }
        Some(Mirror { directory, base })
    }

    /// Every file of the copy outside elfie-compile, elfie-requests, and the copy's own
    /// `.git` file that differs from the base, was added, or was removed, with what the base
    /// held for it and what the copy holds, in path order: the batch's changes, which are
    /// what a merge into the root writes and what its accepted changes are taken from.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    pub(crate) fn changes(&self) -> Changes {
        let mut now: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        // What mode each file of the copy is in, so a file merged into the root keeps whether
        // it is executable.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        let mut modes: BTreeMap<String, Option<u32>> = BTreeMap::new();
        // What the copy is compared with the base over: every path the base holds, which is
        // every path the copy was made from, and every file git lists as added in the copy.
        // A walk of the copy would have to decide for itself which directories hold work and
        // which hold nothing, and a wrong guess reads a file that is there as removed.
        // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
        let mut found: BTreeSet<String> = self.base.keys().cloned().collect();
        // A file the batch added counts only when git would track it.
        // @lfy def/cli/main.lfy:main#main:main:b68cee4a0de41c4715a35c4aa1fd6d7f394b55f3833e77d16dd14f07c3893164
        let added = git(&self.directory, &["ls-files", "--others", "--exclude-standard"]);
        for path in added.unwrap_or_default().lines() {
            let path = path.trim();
            if !path.is_empty() {
                found.insert(path.to_string());
            }
        }
        // The paths of the base the copy no longer holds anything at.
        // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for path in found {
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            if under(&path, &UNMERGED) {
                continue;
            }
            let at = self.directory.join(&path);
            match fs::read(&at) {
                // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
                Ok(held) => {
                    modes.insert(path.clone(), mode_of(&at));
                    now.insert(path, held);
                }
                // A path counts as removed only when nothing is at it in the copy; one that
                // is there and cannot be read is left out of the merge altogether, so the
                // root's file stands rather than going.
                // @lfy def/cli/main.lfy:main#main:main:83b890265a80eabfeb07845b755aca4a5a8ce09a8e540c54a21997798a0c554a
                Err(_) if !at.exists() => {
                    gone.insert(path);
                }
                Err(_) => {}
            }
        }
        // What was added, what differs, and what was removed.
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        let mut paths: BTreeSet<&String> = now.keys().collect();
        paths.extend(gone.iter());
        let mut changes: Changes = BTreeMap::new();
        for path in paths {
            let base = self.base.get(path).cloned();
            let text = now.get(path).cloned();
            // The batch left it as it was: there is nothing of it to merge.
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
            if base.as_deref() == text.as_deref() {
                continue;
            }
            let mode = modes.get(path).copied().flatten();
            // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
            changes.insert(path.clone(), Change { base, text, mode });
        }
        changes
    }

    /// Every change of the batch merged into the root: the paths merged, or the one file
    /// that could not be merged without a conflict.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    pub(crate) fn merge(&self, root: &Path) -> Result<Vec<String>, String> {
        // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
        merge_changes(&self.changes(), root, &self.scratch())
    }

    /// Where the three sides of a merge are written: beside the copy's own cache, which no
    /// merge reads.
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    pub(crate) fn scratch(&self) -> PathBuf {
        self.directory.join("elfie-compile/cache/merge")
    }

    /// What each file of the copy holds now kept as the batch's base: what it is after the
    /// accepted changes it takes are merged in, and before any kept work is taken.
    // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
    pub(crate) fn rebase(&mut self) {
        // @lfy def/cli/main.lfy:main#main:main:1a2d8036f4189865cc6aa7e17b9a517f77ac124aa2c4c40497f8faf1b84c4f17
        for (path, change) in self.changes() {
            match change.text {
                Some(text) => {
                    self.base.insert(path, text);
                }
                None => {
                    self.base.remove(&path);
                }
            }
        }
    }

    /// The copy removed, once its work has reached the root.
    // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
    pub(crate) fn remove(&self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// What one file of a batch's copy holds and what its base held: one of the batch's changes.
// @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Change {
    /// What the base held for it; nothing when the batch added it.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    pub(crate) base: Option<Vec<u8>>,
    /// What the copy holds for it; nothing when the batch removed it.
    // @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
    pub(crate) text: Option<Vec<u8>>,
    /// Whether the copy's file is executable, so the file merged keeps it.
    // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
    pub(crate) mode: Option<u32>,
}

/// The changes of one batch, by path, in path order.
// @lfy def/cli/main.lfy:main#main:main:eceff310959fa29bae3da5b35fd3856bfaccd67f6a0866c09241cd83f43af49b
pub(crate) type Changes = BTreeMap<String, Change>;

/// Every change merged into a directory, file by file: where the directory's file still holds
/// what the base holds it becomes the change's, or is removed when the change removed it, and
/// where something changed it since, the directory's file holds the changes of both, as the
/// three-way merge of the two against the base.
///
/// Nothing is written until every file has merged, so a conflict merges nothing at all, and
/// the one file that could not be merged is what it gives back instead.
// @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
// @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
// @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
pub(crate) fn merge_changes(changes: &Changes, into: &Path, scratch: &Path) -> Result<Vec<String>, String> {
    let mut writes: Vec<(String, Option<Vec<u8>>, Option<u32>)> = Vec::new();
    for (path, change) in changes {
        let theirs = fs::read(into.join(path)).ok();
        // The file still holds what the base holds: it is replaced by the change's text, or
        // removed when the change removed it.
        // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
        if theirs.as_deref() == change.base.as_deref() {
            // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
            writes.push((path.clone(), change.text.clone(), change.mode));
            continue;
        }
        // Something else changed the file since the base was taken: it holds the changes of
        // both, as the three-way merge of it and the change's text against the base. One of
        // the three missing is a file added or removed on both sides, which no merge settles.
        // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
        let (Some(base), Some(mine), Some(theirs)) =
            (change.base.as_deref(), change.text.as_deref(), theirs.as_deref())
        else {
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            return Err(path.clone());
        };
        // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
        match merged(theirs, base, mine, scratch) {
            Some(text) => writes.push((path.clone(), Some(text), change.mode)),
            // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
            None => return Err(path.clone()),
        }
    }
    let mut written = Vec::new();
    for (path, text, mode) in writes {
        let to = into.join(&path);
        match text {
            // @lfy def/cli/main.lfy:main#main:main:6718d622dc82408e32f09610d9066ffc91c81bfa1311cacf0fa3f5d65888f09a
            Some(text) => {
                // @lfy def/cli/main.lfy:main#main:main:97749da3fde520bf9fb6a1d4a7f1782131dc4eea08223f3ea0036e11724b6e42
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::write(&to, &text).is_err() {
                    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
                    return Err(path);
                }
                // The file merged keeps whether the change's was executable.
                // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
                set_mode(&to, mode);
            }
            // @lfy def/cli/main.lfy:main#main:main:c76294da9586f053f15fc0f1023d6594cfd3d0d6be957baa3e1460d2dc5558ac
            None => {
                let _ = fs::remove_file(&to);
            }
        }
        written.push(path);
    }
    Ok(written)
}

/// The three-way merge of two sides against their base, as `git merge-file` gives it;
/// nothing when it conflicts.
// @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
pub(crate) fn merged(theirs: &[u8], base: &[u8], mine: &[u8], scratch: &Path) -> Option<Vec<u8>> {
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    fs::create_dir_all(scratch).ok()?;
    let sides = [("theirs", theirs), ("base", base), ("mine", mine)];
    for (name, held) in sides {
        fs::write(scratch.join(name), held).ok()?;
    }
    // @lfy def/cli/main.lfy:main#main:main:e76ccc6bcfb76d780d95857a4f601ad9cd1c2146c9ae08ef0ec4bc83de1934ff
    let output = Process::new("git")
        .args(["merge-file", "-p", "--quiet", "theirs", "base", "mine"])
        .current_dir(scratch)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let _ = fs::remove_dir_all(scratch);
    // A code of 0 is a clean merge; anything above it counts the conflicts, and anything
    // below it is git itself failing.
    // @lfy def/cli/main.lfy:main#main:main:778beafa50331ee0e4592814d25234b9b53a467f199785957330b905729b79ac
    (output.status.code() == Some(0)).then_some(output.stdout)
}

/// Where elfie-compile keeps the work a rejected batch's copy left behind, so the next
/// compile of the batch starts from it rather than from nothing.
///
/// It lives in the cache, and nothing else does: a command run after the cache is deleted
/// makes the same requests as one run with it, except that a batch that would have taken
/// kept work is handed the outputs the root holds instead.
// @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
// @lfy def/cli/main.lfy:main#main:main:b7b25cb5b407b29319dfe9ee6e8a2d89d37f8d764c616dcbb3ddc2e6a43c0213
pub(crate) const KEPT: &str = "elfie-compile/cache/kept";

/// One unit of a batch as its kept work names it: what it is and what it was asked for, so
/// that work kept for one program is never taken for another.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
pub(crate) struct KeptUnit {
    /// [`Unit::stem`].
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) stem: String,
    /// The SHA-256 of [`Unit::text`] as lowercase hex.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) hash: String,
    /// The SHA-256 of the unit's local criterion and test ids, as
    /// [`SourceMap::requirements`] takes them.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) requirements: String,
}

/// One file of a batch's kept work: what the batch's base held for it and what its copy held.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
pub(crate) struct KeptFile {
    pub(crate) path: String,
    /// What the base held for it; nothing when the batch added it.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) base: Option<String>,
    /// What the copy held for it; nothing when the batch removed it.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) text: Option<String>,
}

/// The work a batch's copy held when a run rejected it and nothing ever accepted it: what it
/// was for, what was wrong with it, and what its files held.
// @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
pub(crate) struct Kept {
    /// [`Target::identifier`] of the batch's target.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) target: String,
    /// Each unit of the batch, in batch order.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) units: Vec<KeptUnit>,
    /// The problems of the rejection that stopped the batch, in the order they were printed.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) problems: Vec<String>,
    /// Each file that would have been merged into the root were the batch done, in path
    /// order.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) files: Vec<KeptFile>,
}

impl Kept {
    /// The kept work of a batch as one JSON object with the keys target, units, problems, and
    /// files.
    // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
    pub(crate) fn to_json(&self) -> serde_json::Value {
        // @lfy def/cli/main.lfy:main#main:main:a34a049f83b7e9534328da95cf00692d300ab1d757fafd5078cdd83e0649728b
        serde_json::json!({
            "target": self.target,
            "units": self
                .units
                .iter()
                .map(|unit| serde_json::json!({
                    "stem": unit.stem,
                    "hash": unit.hash,
                    "requirements": unit.requirements,
                }))
                .collect::<Vec<serde_json::Value>>(),
            "problems": self.problems,
            "files": self
                .files
                .iter()
                .map(|file| serde_json::json!({
                    "path": file.path,
                    "base": file.base,
                    "text": file.text,
                }))
                .collect::<Vec<serde_json::Value>>(),
        })
    }

    /// The kept work one file holds, when it reads as one.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    pub(crate) fn of_json(value: &serde_json::Value) -> Option<Kept> {
        let object = value.as_object()?;
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let units = object
            .get("units")?
            .as_array()?
            .iter()
            .map(|unit| {
                Some(KeptUnit {
                    stem: unit.get("stem")?.as_str()?.to_string(),
                    hash: unit.get("hash")?.as_str()?.to_string(),
                    requirements: unit.get("requirements")?.as_str()?.to_string(),
                })
            })
            .collect::<Option<Vec<KeptUnit>>>()?;
        // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
        let files = object
            .get("files")?
            .as_array()?
            .iter()
            .map(|file| {
                Some(KeptFile {
                    path: file.get("path")?.as_str()?.to_string(),
                    base: file.get("base").and_then(|held| held.as_str()).map(str::to_string),
                    text: file.get("text").and_then(|held| held.as_str()).map(str::to_string),
                })
            })
            .collect::<Option<Vec<KeptFile>>>()?;
        Some(Kept {
            target: object.get("target")?.as_str()?.to_string(),
            units,
            problems: object
                .get("problems")
                .and_then(|held| held.as_array())
                .map(|held| held.iter().filter_map(|one| one.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            files,
        })
    }

    /// Whether the kept work is the batch's own: it names the batch's target and the stems of
    /// its units in batch order, and gives each unit the hash and the requirements that are
    /// its own now.
    // @lfy def/cli/main.lfy:main#main:main:2183852c8ad5bdd852a88254141b828b8d85bc17d90e0d186b2e0d755802e072
    // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
    pub(crate) fn is_for(&self, asked: &Kept) -> bool {
        // @lfy def/cli/main.lfy:main#main:main:e12cb0bab33ffea8be1bf737d72e297e3f70fc95f34b089abc10cc9071b1e079
        self.target == asked.target
            && self.units.len() == asked.units.len()
            && self.units.iter().zip(&asked.units).all(|(held, want)| {
                held.stem == want.stem && held.hash == want.hash && held.requirements == want.requirements
            })
    }

    /// The stems the kept work names.
    // @lfy def/cli/main.lfy:main#main:main:720e04231f491a512ec22a28a29c86eb716470a466f2d9524f3039cefcc815f9
    pub(crate) fn stems(&self) -> Vec<&str> {
        self.units.iter().map(|unit| unit.stem.as_str()).collect()
    }
}

/// The file a batch's kept work is written to, under the root.
// @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
pub(crate) fn kept_file(root: &Path, identifier: &str) -> PathBuf {
    // @lfy def/cli/main.lfy:main#main:main:05313ca78250a3763c29ce16e2df5f9b7e8f73f2327f0cd6bab1ad4a7a024496
    root.join(KEPT).join(format!("{}.json", file_name_of(identifier)))
}

/// Where one batch's compiler, verifier, and outputs are: the root itself, or a copy of it.
// @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
pub(crate) enum Where {
    /// The root itself, which is where a batch runs with --jobs 1 or outside a git
    /// repository, since nothing else is running to break.
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    Root(PathBuf),
    /// Its own copy of the root.
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    Copy(Mirror),
}

impl Where {
    /// Where a batch runs: a copy of the root when more than one batch may run at once and
    /// the root is a git repository, and the root itself otherwise.
    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
    pub(crate) fn of(root: &Path, identifier: &str, copies: bool) -> Where {
        // The batch's directory is the root itself.
        // @lfy def/cli/main.lfy:main#main:main:cab5d4506292d57bf47d916ae73f8496c6abca998665c7cf57c137c631bd51e0
        if !copies {
            return Where::Root(root.to_path_buf());
        }
        // @lfy def/cli/main.lfy:main#main:main:e7f50bd110682bf5ae12f4862ac9abb6bada51f494230119208bc5d4d992f7e6
        match Mirror::take(root, identifier) {
            Some(mirror) => Where::Copy(mirror),
            // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
            None => Where::Root(root.to_path_buf()),
        }
    }

    // @lfy def/cli/main.lfy:main#main:main:081646b2acec666ff388eca1e9495c2fcfdc0ec348b6b36a46623637ff31620f
    pub(crate) fn directory(&self) -> &Path {
        match self {
            Where::Root(root) => root,
            Where::Copy(mirror) => &mirror.directory,
        }
    }
}
