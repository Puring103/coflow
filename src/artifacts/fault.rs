use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Point {
    CreateOutputParent,
    CreateStagingDirectory,
    CreateArtifactParent,
    CreateArtifactFile,
    WriteArtifactFile,
    SyncArtifactFile,
    ReadArtifactFile,
    SyncStagingTree,
    SealGeneration,
    SyncGenerationParent,
    MoveRequestedOutputToBackup,
    PublishRequestedOutput,
    ReadActiveManifest,
    ValidateActiveGeneration,
    WriteEnumLockMirror,
    CreateArtifactStateDirectory,
    WriteActiveManifest,
}

#[cfg(not(test))]
#[inline]
#[allow(clippy::unnecessary_wraps)] // Matches the fault-injecting test implementation.
pub(super) const fn check(_point: Point) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
thread_local! {
    static INJECTED: std::cell::Cell<Option<Point>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn check(point: Point) -> io::Result<()> {
    if INJECTED.with(|injected| injected.get() == Some(point)) {
        Err(io::Error::other(format!(
            "injected artifact filesystem failure at {point:?}"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct InjectionGuard;

#[cfg(test)]
impl Drop for InjectionGuard {
    fn drop(&mut self) {
        INJECTED.with(|injected| injected.set(None));
    }
}

#[cfg(test)]
pub(super) fn inject(point: Point) -> InjectionGuard {
    INJECTED.with(|injected| {
        assert!(
            injected.replace(Some(point)).is_none(),
            "nested fault injection"
        );
    });
    InjectionGuard
}

#[cfg(test)]
pub(super) const ALL_POINTS: [Point; 17] = [
    Point::CreateOutputParent,
    Point::CreateStagingDirectory,
    Point::CreateArtifactParent,
    Point::CreateArtifactFile,
    Point::WriteArtifactFile,
    Point::SyncArtifactFile,
    Point::ReadArtifactFile,
    Point::SyncStagingTree,
    Point::SealGeneration,
    Point::SyncGenerationParent,
    Point::MoveRequestedOutputToBackup,
    Point::PublishRequestedOutput,
    Point::ReadActiveManifest,
    Point::ValidateActiveGeneration,
    Point::WriteEnumLockMirror,
    Point::CreateArtifactStateDirectory,
    Point::WriteActiveManifest,
];
