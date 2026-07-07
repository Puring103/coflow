use serde_json::Value;

pub trait LarkHttpClient {
    /// Performs a Feishu/Lark authenticated GET request.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn get(&self, url: &str, tenant_access_token: &str) -> Result<String, String>;

    /// Performs a Feishu/Lark JSON POST request.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn post_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: Option<&str>,
    ) -> Result<String, String>;

    /// Performs an authenticated PUT request with a JSON body. Writers use
    /// this for batch update endpoints; the default implementation routes
    /// through `post_json` so existing fakes only need to implement two
    /// methods, but real clients should override for correct semantics.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn put_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: &str,
    ) -> Result<String, String> {
        self.post_json(url, body, Some(tenant_access_token))
    }

    /// Performs an authenticated DELETE request with a JSON body.
    ///
    /// # Errors
    ///
    /// Returns a transport or HTTP response error message.
    fn delete_json(
        &self,
        _url: &str,
        _body: &Value,
        _tenant_access_token: &str,
    ) -> Result<String, String> {
        Err("DELETE with JSON body is not implemented by this Lark HTTP client".to_string())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UreqLarkHttpClient;

impl LarkHttpClient for UreqLarkHttpClient {
    fn get(&self, url: &str, tenant_access_token: &str) -> Result<String, String> {
        ureq::get(url)
            .set("Authorization", &format!("Bearer {tenant_access_token}"))
            .call()
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }

    fn post_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: Option<&str>,
    ) -> Result<String, String> {
        let mut request = ureq::post(url).set("Content-Type", "application/json");
        let bearer;
        if let Some(token) = tenant_access_token {
            bearer = format!("Bearer {token}");
            request = request.set("Authorization", &bearer);
        }
        request
            .send_string(&body.to_string())
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }

    fn put_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: &str,
    ) -> Result<String, String> {
        ureq::put(url)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {tenant_access_token}"))
            .send_string(&body.to_string())
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }

    fn delete_json(
        &self,
        url: &str,
        body: &Value,
        tenant_access_token: &str,
    ) -> Result<String, String> {
        ureq::delete(url)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {tenant_access_token}"))
            .send_string(&body.to_string())
            .map_err(ureq_error_message)?
            .into_string()
            .map_err(|err| err.to_string())
    }
}

fn ureq_error_message(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, response) => {
            let status = response.status_text().to_string();
            match response.into_string() {
                Ok(body) if !body.is_empty() => {
                    format!("HTTP {code} {status}: {body}")
                }
                _ => format!("HTTP {code} {status}"),
            }
        }
        ureq::Error::Transport(err) => err.to_string(),
    }
}
