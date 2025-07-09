//! Utility macros for the pipeline crate.

/// A macro to simplify the handling of control messages in a stage's run loop.
///
/// This macro processes all pending messages from the control receiver,
/// handling common messages like `Pause` and `Resume` automatically. It
/// delegates `UpdateParam` messages to the provided stage's `update_param`
/// method.
#[macro_export]
macro_rules! ctrl_loop {
    ($self:ident, $ctx:ident) => {
        while let Ok(msg) = $ctx.control_rx.try_recv() {
            match msg {
                $crate::stage::ControlMsg::Pause => {
                    tracing::trace!("Stage paused");
                    $self.enabled.store(false, std::sync::atomic::Ordering::Release);
                }
                $crate::stage::ControlMsg::Resume => {
                    tracing::trace!("Stage resumed");
                    $self.enabled.store(true, std::sync::atomic::Ordering::Release);
                }
                $crate::stage::ControlMsg::UpdateParam(key, val) => {
                    if let Err(e) = $self.update_param(&key, val) {
                        tracing::error!("Failed to update parameter: {}", e);
                    }
                }
                _ => {
                    tracing::warn!("Received unknown control message: {:?}", msg);
                }
            }
        }
    };
}