use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::net::{IpAddr, SocketAddr};
use std::fmt;
use std::str;

use yansi::Paint;
use state::{Container, Storage};

use super::{FromParam, FromSegments, FromRequest, Outcome};

use rocket::Rocket;
use router::Route;
use config::{Config, Limits};
use http::uri::{Origin, Segments};
use error::Error;
use http::{Method, Header, HeaderMap, Cookies, CookieJar};
use http::{RawStr, ContentType, Accept, MediaType};
use http::hyper;

#[derive(Clone)]
struct RequestState<'r> {
    config: &'r Config,
    managed: &'r Container,
    params: RefCell<Vec<(usize, usize)>>,
    route: Cell<Option<&'r Route>>,
    cookies: RefCell<CookieJar>,
    accept: Storage<Option<Accept>>,
    content_type: Storage<Option<ContentType>>,
    cache: Rc<Container>,
}

/// The type of an incoming web request.
///
/// This should be used sparingly in Rocket applications. In particular, it
/// should likely only be used when writing
/// [FromRequest](/rocket/request/trait.FromRequest.html) implementations. It
/// contains all of the information for a given web request except for the body
/// data. This includes the HTTP method, URI, cookies, headers, and more.
#[derive(Clone)]
pub struct Request<'r> {
    method: Cell<Method>,
    uri: Origin<'r>,
    headers: HeaderMap<'r>,
    remote: Option<SocketAddr>,
    state: RequestState<'r>,
}

impl<'r> Request<'r> {
    /// Create a new `Request` with the given `method` and `uri`.
    #[inline(always)]
    crate fn new<'s: 'r>(
        rocket: &'r Rocket,
        method: Method,
        uri: Origin<'s>
    ) -> Request<'r> {
        Request {
            method: Cell::new(method),
            uri: uri,
            headers: HeaderMap::new(),
            remote: None,
            state: RequestState {
                config: &rocket.config,
                managed: &rocket.state,
                route: Cell::new(None),
                params: RefCell::new(Vec::new()),
                cookies: RefCell::new(CookieJar::new()),
                accept: Storage::new(),
                content_type: Storage::new(),
                cache: Rc::new(Container::new()),
            }
        }
    }

    // Only used by doc-tests!
    #[doc(hidden)]
    pub fn example<F: Fn(&mut Request)>(method: Method, uri: &str, f: F) {
        let rocket = Rocket::custom(Config::development().unwrap());
        let uri = Origin::parse(uri).expect("invalid URI in example");
        let mut request = Request::new(&rocket, method, uri);
        f(&mut request);
    }

    /// Retrieve the method from `self`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// use rocket::http::Method;
    ///
    /// # Request::example(Method::Get, "/uri", |request| {
    /// request.set_method(Method::Get);
    /// assert_eq!(request.method(), Method::Get);
    /// # });
    /// ```
    #[inline(always)]
    pub fn method(&self) -> Method {
        self.method.get()
    }

    /// Set the method of `self`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// use rocket::http::Method;
    ///
    /// # Request::example(Method::Get, "/uri", |request| {
    /// assert_eq!(request.method(), Method::Get);
    ///
    /// request.set_method(Method::Post);
    /// assert_eq!(request.method(), Method::Post);
    /// # });
    /// ```
    #[inline(always)]
    pub fn set_method(&mut self, method: Method) {
        self._set_method(method);
    }

    /// Borrow the `Origin` URI from `self`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// # Request::example(Method::Get, "/uri", |request| {
    /// assert_eq!(request.uri().path(), "/uri");
    /// # });
    /// ```
    #[inline(always)]
    pub fn uri(&self) -> &Origin {
        &self.uri
    }

    /// Set the URI in `self` to `uri`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rocket::http::uri::Origin;
    ///
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// let uri = Origin::parse("/hello/Sergio?type=greeting").unwrap();
    ///
    /// request.set_uri(uri);
    /// assert_eq!(request.uri().path(), "/hello/Sergio");
    /// assert_eq!(request.uri().query(), Some("type=greeting"));
    /// # });
    /// ```
    #[inline(always)]
    pub fn set_uri<'u: 'r>(&mut self, uri: Origin<'u>) {
        self.uri = uri;
        *self.state.params.borrow_mut() = Vec::new();
    }

    /// Returns the address of the remote connection that initiated this
    /// request if the address is known. If the address is not known, `None` is
    /// returned.
    ///
    /// Because it is common for proxies to forward connections for clients, the
    /// remote address may contain information about the proxy instead of the
    /// client. For this reason, proxies typically set the "X-Real-IP" header
    /// with the client's true IP. To extract this IP from the request, use the
    /// [`real_ip()`] or [`client_ip()`] methods.
    ///
    /// [`real_ip()`]: #method.real_ip
    /// [`client_ip()`]: #method.client_ip
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// # Request::example(Method::Get, "/uri", |request| {
    /// assert!(request.remote().is_none());
    /// # });
    /// ```
    #[inline(always)]
    pub fn remote(&self) -> Option<SocketAddr> {
        self.remote
    }

    /// Sets the remote address of `self` to `address`.
    ///
    /// # Example
    ///
    /// Set the remote address to be 127.0.0.1:8000:
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// let (ip, port) = (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000);
    /// let localhost = SocketAddr::new(ip, port);
    /// request.set_remote(localhost);
    ///
    /// assert_eq!(request.remote(), Some(localhost));
    /// # });
    /// ```
    #[inline(always)]
    pub fn set_remote(&mut self, address: SocketAddr) {
        self.remote = Some(address);
    }

    /// Returns the IP address in the "X-Real-IP" header of the request if such
    /// a header exists and contains a valid IP address.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::{Header, Method};
    /// # use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// request.add_header(Header::new("X-Real-IP", "8.8.8.8"));
    /// assert_eq!(request.real_ip(), Some("8.8.8.8".parse().unwrap()));
    /// # });
    /// ```
    pub fn real_ip(&self) -> Option<IpAddr> {
        self.headers()
            .get_one("X-Real-IP")
            .and_then(|ip| {
                ip.parse()
                    .map_err(|_| warn_!("'X-Real-IP' header is malformed: {}", ip))
                    .ok()
            })
    }

    /// Attempts to return the client's IP address by first inspecting the
    /// "X-Real-IP" header and then using the remote connection's IP address.
    ///
    /// If the "X-Real-IP" header exists and contains a valid IP address, that
    /// address is returned. Otherwise, if the address of the remote connection
    /// is known, that address is returned. Otherwise, `None` is returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::{Header, Method};
    /// # use std::net::{SocketAddr, IpAddr, Ipv4Addr};
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// // starting without an "X-Real-IP" header or remote addresss
    /// assert!(request.client_ip().is_none());
    ///
    /// // add a remote address; this is done by Rocket automatically
    /// request.set_remote("127.0.0.1:8000".parse().unwrap());
    /// assert_eq!(request.client_ip(), Some("127.0.0.1".parse().unwrap()));
    ///
    /// // now with an X-Real-IP header
    /// request.add_header(Header::new("X-Real-IP", "8.8.8.8"));
    /// assert_eq!(request.client_ip(), Some("8.8.8.8".parse().unwrap()));
    /// # });
    /// ```
    #[inline]
    pub fn client_ip(&self) -> Option<IpAddr> {
        self.real_ip().or_else(|| self.remote().map(|r| r.ip()))
    }

    /// Returns a [`HeaderMap`](/rocket/http/struct.HeaderMap.html) of all of
    /// the headers in `self`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// # Request::example(Method::Get, "/uri", |request| {
    /// let header_map = request.headers();
    /// assert!(header_map.is_empty());
    /// # });
    /// ```
    #[inline(always)]
    pub fn headers(&self) -> &HeaderMap<'r> {
        &self.headers
    }

    /// Add `header` to `self`'s headers. The type of `header` can be any type
    /// that implements the `Into<Header>` trait. This includes common types
    /// such as [`ContentType`](/rocket/http/struct.ContentType.html) and
    /// [`Accept`](/rocket/http/struct.Accept.html).
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// use rocket::http::ContentType;
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// assert!(request.headers().is_empty());
    ///
    /// request.add_header(ContentType::HTML);
    /// assert!(request.headers().contains("Content-Type"));
    /// assert_eq!(request.headers().len(), 1);
    /// # });
    /// ```
    #[inline(always)]
    pub fn add_header<'h: 'r, H: Into<Header<'h>>>(&mut self, header: H) {
        self.headers.add(header.into());
    }

    /// Replaces the value of the header with name `header.name` with
    /// `header.value`. If no such header exists, `header` is added as a header
    /// to `self`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// use rocket::http::ContentType;
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// assert!(request.headers().is_empty());
    ///
    /// request.add_header(ContentType::Any);
    /// assert_eq!(request.headers().get_one("Content-Type"), Some("*/*"));
    ///
    /// request.replace_header(ContentType::PNG);
    /// assert_eq!(request.headers().get_one("Content-Type"), Some("image/png"));
    /// # });
    /// ```
    #[inline(always)]
    pub fn replace_header<'h: 'r, H: Into<Header<'h>>>(&mut self, header: H) {
        self.headers.replace(header.into());
    }

    /// Returns a wrapped borrow to the cookies in `self`.
    ///
    /// [`Cookies`](/rocket/http/enum.Cookies.html) implements internal
    /// mutability, so this method allows you to get _and_ add/remove cookies in
    /// `self`.
    ///
    /// # Example
    ///
    /// Add a new cookie to a request's cookies:
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// use rocket::http::Cookie;
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// request.cookies().add(Cookie::new("key", "val"));
    /// request.cookies().add(Cookie::new("ans", format!("life: {}", 38 + 4)));
    /// # });
    /// ```
    pub fn cookies(&self) -> Cookies {
        // FIXME: Can we do better? This is disappointing.
        match self.state.cookies.try_borrow_mut() {
            Ok(jar) => Cookies::new(jar, self.state.config.secret_key()),
            Err(_) => {
                error_!("Multiple `Cookies` instances are active at once.");
                info_!("An instance of `Cookies` must be dropped before another \
                       can be retrieved.");
                warn_!("The retrieved `Cookies` instance will be empty.");
                Cookies::empty()
            }
        }
    }

    /// Returns the Content-Type header of `self`. If the header is not present,
    /// returns `None`. The Content-Type header is cached after the first call
    /// to this function. As a result, subsequent calls will always return the
    /// same value.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// use rocket::http::ContentType;
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// request.add_header(ContentType::JSON);
    /// assert_eq!(request.content_type(), Some(&ContentType::JSON));
    ///
    /// // The header is cached; it cannot be replaced after first access.
    /// request.replace_header(ContentType::HTML);
    /// assert_eq!(request.content_type(), Some(&ContentType::JSON));
    /// # });
    /// ```
    #[inline(always)]
    pub fn content_type(&self) -> Option<&ContentType> {
        self.state.content_type.get_or_set(|| {
            self.headers().get_one("Content-Type").and_then(|v| v.parse().ok())
        }).as_ref()
    }

    /// Returns the Accept header of `self`. If the header is not present,
    /// returns `None`. The Accept header is cached after the first call to this
    /// function. As a result, subsequent calls will always return the same
    /// value.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// use rocket::http::Accept;
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// request.add_header(Accept::JSON);
    /// assert_eq!(request.accept(), Some(&Accept::JSON));
    ///
    /// // The header is cached; it cannot be replaced after first access.
    /// request.replace_header(Accept::HTML);
    /// assert_eq!(request.accept(), Some(&Accept::JSON));
    /// # });
    /// ```
    #[inline(always)]
    pub fn accept(&self) -> Option<&Accept> {
        self.state.accept.get_or_set(|| {
            self.headers().get_one("Accept").and_then(|v| v.parse().ok())
        }).as_ref()
    }

    /// Returns the media type "format" of the request.
    ///
    /// The "format" of a request is either the Content-Type, if the request
    /// methods indicates support for a payload, or the preferred media type in
    /// the Accept header otherwise. If the method indicates no payload and no
    /// Accept header is specified, a media type of `Any` is returned.
    ///
    /// The media type returned from this method is used to match against the
    /// `format` route attribute.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// use rocket::http::{Method, Accept, ContentType, MediaType};
    ///
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// request.add_header(ContentType::JSON);
    /// request.add_header(Accept::HTML);
    ///
    /// request.set_method(Method::Get);
    /// assert_eq!(request.format(), Some(&MediaType::HTML));
    ///
    /// request.set_method(Method::Post);
    /// assert_eq!(request.format(), Some(&MediaType::JSON));
    /// # });
    /// ```
    pub fn format(&self) -> Option<&MediaType> {
        static ANY: MediaType = MediaType::Any;
        if self.method().supports_payload() {
            self.content_type().map(|ct| ct.media_type())
        } else {
            // FIXME: Should we be using `accept_first` or `preferred`? Or
            // should we be checking neither and instead pass things through
            // where the client accepts the thing at all?
            self.accept()
                .map(|accept| accept.preferred().media_type())
                .or(Some(&ANY))
        }
    }

    /// Returns the configured application receive limits.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// let json_limit = request.limits().get("json");
    /// # });
    /// ```
    pub fn limits(&self) -> &'r Limits {
        &self.state.config.limits
    }

    /// Get the presently matched route, if any.
    ///
    /// This method returns `Some` any time a handler or its guards are being
    /// invoked. This method returns `None` _before_ routing has commenced; this
    /// includes during request fairing callbacks.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::Request;
    /// # use rocket::http::Method;
    /// # Request::example(Method::Get, "/uri", |mut request| {
    /// let route = request.route();
    /// # });
    /// ```
    pub fn route(&self) -> Option<&'r Route> {
        self.state.route.get()
    }

    /// Invokes the request guard implementation for `T`, returning its outcome.
    ///
    /// # Example
    ///
    /// Assuming a `User` request guard exists, invoke it:
    ///
    /// ```rust,ignore
    /// let outcome = request.guard::<User>();
    /// ```
    ///
    /// Retrieve managed state inside of a guard implementation:
    ///
    /// ```rust,ignore
    /// use rocket::State;
    ///
    /// let pool = request.guard::<State<Pool>>()?;
    /// ```
    #[inline(always)]
    pub fn guard<'a, T: FromRequest<'a, 'r>>(&'a self) -> Outcome<T, T::Error> {
        T::from_request(self)
    }

    /// Retrieves the cached value for type `T` from the request-local cached
    /// state of `self`. If no such value has previously been cached for this
    /// request, `f` is called to produce the value which is subsequently
    /// returned.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket::http::Method;
    /// # use rocket::Request;
    /// # struct User;
    /// fn current_user(request: &Request) -> User {
    ///     // Validate request for a given user, load from database, etc.
    ///     # User
    /// }
    ///
    /// # Request::example(Method::Get, "/uri", |request| {
    /// let user = request.local_cache(|| current_user(request));
    /// # });
    /// ```
    pub fn local_cache<T, F>(&self, f: F) -> &T
        where F: FnOnce() -> T,
              T: Send + Sync + 'static
    {
        self.state.cache.try_get()
            .unwrap_or_else(|| {
                self.state.cache.set(f());
                self.state.cache.get()
            })
    }

    /// Retrieves and parses into `T` the 0-indexed `n`th dynamic parameter from
    /// the request. Returns `Error::NoKey` if `n` is greater than the number of
    /// params. Returns `Error::BadParse` if the parameter type `T` can't be
    /// parsed from the parameter.
    ///
    /// This method exists only to be used by manual routing. To retrieve
    /// parameters from a request, use Rocket's code generation facilities.
    ///
    /// # Example
    ///
    /// Retrieve parameter `0`, which is expected to be a `String`, in a manual
    /// route:
    ///
    /// ```rust
    /// use rocket::{Request, Data};
    /// use rocket::handler::Outcome;
    ///
    /// # #[allow(dead_code)]
    /// fn name<'a>(req: &'a Request, _: Data) -> Outcome<'a> {
    ///     Outcome::from(req, req.get_param::<String>(0).unwrap_or("unnamed".into()))
    /// }
    /// ```
    pub fn get_param<'a, T: FromParam<'a>>(&'a self, n: usize) -> Result<T, Error> {
        let param = self.get_param_str(n).ok_or(Error::NoKey)?;
        T::from_param(param).map_err(|_| Error::BadParse)
    }

    /// Get the `n`th path parameter as a string, if it exists. This is used by
    /// codegen.
    #[doc(hidden)]
    pub fn get_param_str(&self, n: usize) -> Option<&RawStr> {
        let params = self.state.params.borrow();
        if n >= params.len() {
            debug!("{} is >= param count {}", n, params.len());
            return None;
        }

        let (i, j) = params[n];
        let path = self.uri.path();
        if j > path.len() {
            error!("Couldn't retrieve parameter: internal count incorrect.");
            return None;
        }

        Some(path[i..j].into())
    }

    /// Retrieves and parses into `T` all of the path segments in the request
    /// URI beginning at the 0-indexed `n`th dynamic parameter. `T` must
    /// implement [FromSegments](/rocket/request/trait.FromSegments.html), which
    /// is used to parse the segments.
    ///
    /// This method exists only to be used by manual routing. To retrieve
    /// segments from a request, use Rocket's code generation facilities.
    ///
    /// # Error
    ///
    /// If there are less than `n` segments, returns an `Err` of `NoKey`. If
    /// parsing the segments failed, returns an `Err` of `BadParse`.
    ///
    /// # Example
    ///
    /// If the request URI is `"/hello/there/i/am/here"`, and the matched route
    /// path for this request is `"/hello/<name>/i/<segs..>"`, then
    /// `request.get_segments::<T>(1)` will attempt to parse the segments
    /// `"am/here"` as type `T`.
    pub fn get_segments<'a, T: FromSegments<'a>>(&'a self, n: usize)
            -> Result<T, Error> {
        let segments = self.get_raw_segments(n).ok_or(Error::NoKey)?;
        T::from_segments(segments).map_err(|_| Error::BadParse)
    }

    /// Get the segments beginning at the `n`th dynamic parameter, if they
    /// exist. Used by codegen.
    #[doc(hidden)]
    pub fn get_raw_segments(&self, n: usize) -> Option<Segments> {
        let params = self.state.params.borrow();
        if n >= params.len() {
            debug!("{} is >= param (segments) count {}", n, params.len());
            return None;
        }

        let (i, j) = params[n];
        let path = self.uri.path();
        if j > path.len() {
            error!("Couldn't retrieve segments: internal count incorrect.");
            return None;
        }

        Some(Segments(&path[i..j]))
    }

    /// Set `self`'s parameters given that the route used to reach this request
    /// was `route`. This should only be used internally by `Rocket` as improper
    /// use may result in out of bounds indexing.
    /// TODO: Figure out the mount path from here.
    #[inline]
    crate fn set_route(&self, route: &'r Route) {
        self.state.route.set(Some(route));
        *self.state.params.borrow_mut() = route.get_param_indexes(self.uri());
    }

    /// Set the method of `self`, even when `self` is a shared reference.
    #[inline(always)]
    crate fn _set_method(&self, method: Method) {
        self.method.set(method);
    }

    /// Replace all of the cookies in `self` with those in `jar`.
    #[inline]
    crate fn set_cookies(&mut self, jar: CookieJar) {
        self.state.cookies = RefCell::new(jar);
    }

    /// Get the managed state T, if it exists. For internal use only!
    #[inline(always)]
    crate fn get_state<T: Send + Sync + 'static>(&self) -> Option<&'r T> {
        self.state.managed.try_get()
    }

    /// Convert from Hyper types into a Rocket Request.
    crate fn from_hyp(
        rocket: &'r Rocket,
        h_method: hyper::Method,
        h_headers: hyper::header::Headers,
        h_uri: hyper::RequestUri,
        h_addr: SocketAddr,
    ) -> Result<Request<'r>, String> {
        // Get a copy of the URI for later use.
        let uri = match h_uri {
            hyper::RequestUri::AbsolutePath(s) => s,
            _ => return Err(format!("Bad URI: {}", h_uri)),
        };

        // Ensure that the method is known. TODO: Allow made-up methods?
        let method = match Method::from_hyp(&h_method) {
            Some(method) => method,
            None => return Err(format!("Invalid method: {}", h_method))
        };

        // We need to re-parse the URI since we don't trust Hyper... :(
        let uri = Origin::parse_owned(uri).map_err(|e| e.to_string())?;

        // Construct the request object.
        let mut request = Request::new(rocket, method, uri);
        request.set_remote(h_addr);

        // Set the request cookies, if they exist.
        if let Some(cookie_headers) = h_headers.get_raw("Cookie") {
            let mut cookie_jar = CookieJar::new();
            for header in cookie_headers {
                let raw_str = match ::std::str::from_utf8(header) {
                    Ok(string) => string,
                    Err(_) => continue
                };

                for cookie_str in raw_str.split(';').map(|s| s.trim()) {
                    if let Some(cookie) = Cookies::parse_cookie(cookie_str) {
                        cookie_jar.add_original(cookie);
                    }
                }
            }

            request.set_cookies(cookie_jar);
        }

        // Set the rest of the headers.
        for hyp in h_headers.iter() {
            if let Some(header_values) = h_headers.get_raw(hyp.name()) {
                for value in header_values {
                    // This is not totally correct since values needn't be UTF8.
                    let value_str = String::from_utf8_lossy(value).into_owned();
                    let header = Header::new(hyp.name().to_string(), value_str);
                    request.add_header(header);
                }
            }
        }

        Ok(request)
    }
}

impl<'r> fmt::Debug for Request<'r> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Request")
            .field("method", &self.method)
            .field("uri", &self.uri)
            .field("headers", &self.headers())
            .field("remote", &self.remote())
            .finish()
    }
}

impl<'r> fmt::Display for Request<'r> {
    /// Pretty prints a Request. This is primarily used by Rocket's logging
    /// infrastructure.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", Paint::green(self.method()), Paint::blue(&self.uri))?;

        // Print the requests media type when the route specifies a format.
        if let Some(media_type) = self.format() {
            if !media_type.is_any() {
                write!(f, " {}", Paint::yellow(media_type))?;
            }
        }

        Ok(())
    }
}
