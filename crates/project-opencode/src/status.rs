#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendStatus {
    Stopped,
    Starting,
    Ready,
    Failed,
}
