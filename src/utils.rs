use std::sync::Arc;

use futures_util::Future;
use serde::de;

use crate::{error::RustyError, model::ApiError};

/// Implemented by structs that don't need to be wrapped by [`Arc`].
///
/// Internally, there's a reference that points to the internal implementation,
/// protected by [`Arc`].
pub trait InnerArc {
    /// Ref that points to the actual instance.
    type Ref;

    /// Returns the inner instance.
    fn instance(&self) -> &Arc<Self::Ref>;
}

/// Takes a request, waits for its execution and parses its json body.
///
/// If status code isn't between 200 and 299, tries to parse the [`ApiError`],
pub(crate) async fn process_request<T, R>(
    request: R,
) -> Result<T, RustyError>
where
    T: de::DeserializeOwned,
    R: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    match request.await {
        Ok(response) => {
            if (200..=299).contains(&response.status().as_u16()) {
                return match response.json::<T>().await {
                    Ok(content) => Ok(content),
                    Err(e) => Err(RustyError::ParseResponseError(e)),
                };
            }

            // Tries to parse the API error.
            match response.json::<ApiError>().await {
                Ok(api_e) => Err(RustyError::InstanceError(api_e)),
                Err(e) => Err(RustyError::ParseResponseError(e)),
            }
        }
        Err(e) => Err(RustyError::RequestError(e)),
    }
}
