use std::borrow::Cow;

use mdict_core::util::percent_decode;
use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, UriSchemeContext, Wry};

use crate::AppState;

/// Serves `mdict://localhost/<dict-id>/<resource>` (Linux/macOS) and
/// `http://mdict.localhost/<dict-id>/<resource>` (Windows).
pub fn handle(
    ctx: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    match serve(ctx, &request) {
        Ok(response) => response,
        Err((status, message)) => text(status, message),
    }
}

fn serve(
    ctx: UriSchemeContext<'_, Wry>,
    request: &Request<Vec<u8>>,
) -> std::result::Result<Response<Cow<'static, [u8]>>, (StatusCode, String)> {
    let state = ctx.app_handle().state::<AppState>();
    let path = percent_decode(request.uri().path());
    let path = path.trim_start_matches('/');
    let Some((id, resource)) = path.split_once('/') else {
        return Err((StatusCode::NOT_FOUND, "资源路径无效".to_string()));
    };
    if id.is_empty() || resource.is_empty() {
        return Err((StatusCode::NOT_FOUND, "资源路径无效".to_string()));
    }

    match state.store.read_resource(id, resource) {
        Ok(Some((bytes, mime))) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", mime)
            .header("Cache-Control", "public, max-age=3600")
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Owned(bytes))
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())),
        Ok(None) => Err((StatusCode::NOT_FOUND, format!("资源不存在: {resource}"))),
        Err(error) => {
            log::warn!("resource {id}/{resource}: {error}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
        }
    }
}

fn text(status: StatusCode, message: String) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Owned(message.into_bytes()))
        .unwrap_or_else(|_| Response::new(Cow::Borrowed(b"error".as_slice())))
}
