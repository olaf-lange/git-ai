use crate::config;
use crate::error::GitAiError;
use url::Url;

/// API client context with optional authentication
#[derive(Debug, Clone)]
pub struct ApiContext {
    /// Base URL for the API (e.g., "https://app.com")
    pub base_url: String,
    /// Optional authentication token
    pub auth_token: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: Option<u64>,
}

impl ApiContext {
    /// Get the default API base URL from config
    fn default_base_url() -> String {
        config::Config::get().api_base_url().to_string()
    }

    /// Create a new API context without authentication
    /// If base_url is None, uses api_base_url from config (which can be set via config file, env var, or defaults)
    pub fn new(base_url: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(Self::default_base_url),
            auth_token: None,
            timeout_secs: Some(30),
        }
    }

    /// Create a new API context with authentication
    /// If base_url is None, uses api_base_url from config (which can be set via config file, env var, or defaults)
    pub fn with_auth(base_url: Option<String>, auth_token: String) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(Self::default_base_url),
            auth_token: Some(auth_token),
            timeout_secs: Some(30),
        }
    }

    /// Set a custom timeout
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }

    /// Build the full URL for an endpoint
    fn build_url(&self, endpoint: &str) -> Result<String, GitAiError> {
        let base = Url::parse(&self.base_url)
            .map_err(|e| GitAiError::Generic(format!("Invalid base URL: {}", e)))?;
        let url = base
            .join(endpoint)
            .map_err(|e| GitAiError::Generic(format!("Invalid endpoint URL: {}", e)))?;
        Ok(url.to_string())
    }

    /// Make a POST request with JSON body
    pub fn post_json<T: serde::Serialize>(
        &self,
        endpoint: &str,
        body: &T,
    ) -> Result<minreq::Response, GitAiError> {
        let url = self.build_url(endpoint)?;
        let body_json = serde_json::to_string(body)
            .map_err(|e| GitAiError::JsonError(e))?;

        let mut request = minreq::post(&url)
            .with_header("Content-Type", "application/json")
            .with_body(body_json);

        // Add authentication header if token is present
        if let Some(token) = &self.auth_token {
            request = request.with_header("Authorization", format!("Bearer {}", token));
        }

        // Add User-Agent header
        let user_agent = format!("git-ai/{}", env!("CARGO_PKG_VERSION"));
        request = request.with_header("User-Agent", user_agent);

        // Set timeout if specified
        if let Some(timeout) = self.timeout_secs {
            request = request.with_timeout(timeout);
        }

        let response = request
            .send()
            .map_err(|e| GitAiError::Generic(format!("HTTP request failed: {}", e)))?;

        Ok(response)
    }

    /// Make a GET request
    pub fn get(&self, endpoint: &str) -> Result<minreq::Response, GitAiError> {
        let url = self.build_url(endpoint)?;

        let mut request = minreq::get(&url);

        // Add authentication header if token is present
        if let Some(token) = &self.auth_token {
            request = request.with_header("Authorization", format!("Bearer {}", token));
        }

        // Add User-Agent header
        let user_agent = format!("git-ai/{}", env!("CARGO_PKG_VERSION"));
        request = request.with_header("User-Agent", user_agent);

        // Set timeout if specified
        if let Some(timeout) = self.timeout_secs {
            request = request.with_timeout(timeout);
        }

        let response = request
            .send()
            .map_err(|e| GitAiError::Generic(format!("HTTP request failed: {}", e)))?;

        Ok(response)
    }
}

/// API client wrapper
#[derive(Debug, Clone)]
pub struct ApiClient {
    context: ApiContext,
}

impl ApiClient {
    /// Create a new API client with the given context
    pub fn new(context: ApiContext) -> Self {
        Self { context }
    }

    /// Get a reference to the API context
    pub fn context(&self) -> &ApiContext {
        &self.context
    }

    /// Get a mutable reference to the API context
    pub fn context_mut(&mut self) -> &mut ApiContext {
        &mut self.context
    }
}

