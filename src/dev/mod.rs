//! Dev-only 工具: savestate + replay recorder. 仅 dev-tools feature 编译.
//!
//! - savestate: F5/F9 把当前 GameState dump/load JSON. 用于"跳到那一刻".
//! - recorder:  开关打开后每局自动录决策序列, 局结算时 flush. 用于"看 bug 怎么演变".

pub mod recorder;
pub mod savestate;
