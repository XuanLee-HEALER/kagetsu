//! Controller — 标识一个 seat 由真人还是 AI 控制.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controller {
    Human,
    DummyAi,
}
