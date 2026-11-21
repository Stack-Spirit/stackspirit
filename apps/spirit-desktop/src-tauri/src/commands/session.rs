//! Session ownership command adapters.

use spirit_core::vowp::SessionTakeoverResult;
use tauri::State;

use crate::error::CommandResult;
use crate::state::AppState;

/// Takes a session over for StackSpirit.
///
/// The renderer supplies only the session id. The local node and surface label come
/// from the host-side local-node state, and the idle-versus-running confirmation is a
/// UI decision made before this call. A VOWP without the command returns a typed
/// `not_supported` error and no ownership is faked.
#[tauri::command]
pub async fn session_takeover(
    state: State<'_, AppState>,
    session_id: String,
) -> CommandResult<SessionTakeoverResult> {
    let client = state.client()?;
    let core = state.core().await;
    core.session_takeover(client.as_ref(), &session_id).await
}
