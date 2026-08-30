use crate::TunnelResult;
use crate::model::{LocalOrigin, TunnelSession, TunnelState};

pub trait TunnelProvider: Send {
    fn start(&mut self, origin: LocalOrigin) -> TunnelResult<TunnelSession>;
    fn session(&self) -> Option<TunnelSession>;
    fn state(&self) -> TunnelState;
    fn stop(&mut self) -> TunnelResult<()>;
    fn is_running(&self) -> bool;
}
