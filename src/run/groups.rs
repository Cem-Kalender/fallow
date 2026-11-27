//! 018: builders over groups of `[P]` tasks (research R-5, R-6, R-11). A group is a run of
//! consecutive `[P]` tasks in one phase of the locked `tasks.md`, at least two, in a pipeline
//! whose `[caps] builders` is more than 1; each of its tasks is built in its own working copy and
//! merged into the build branch in `tasks.md` order after the touched-files check (R-6). The lock
//! statement (R-11) says at the lock what the builders will do.
