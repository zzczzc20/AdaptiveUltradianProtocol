//! FlowArc 核心库（AUP 2.0）。
//!
//! 所有与时间相关的函数都显式接收 `now`（Unix 秒），因此状态机与规则引擎是纯函数式、可确定性测试的。

pub mod model;
pub mod rules;
pub mod session;
pub mod storage;

pub use model::*;
pub use session::{Engine, EngineState, Phase};
pub use storage::Storage;
