use axum::{extract::Path, response::Redirect, Extension};



#[derive(Debug)]
pub struct AppError(&'static str);

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = match self.0 {
            "not_found" => axum::http::StatusCode::NOT_FOUND,
            _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.0).into_response()
    }
}

// pub async fn redirect_handler(
//     Path(path): Path<String>,
//     Extension(base_url): Extension<String>,
// ) -> Result<Redirect, AppError> {
//     let url =;
//     let redirect = Redirect::to(&url);
//     Ok(redirect)
// }