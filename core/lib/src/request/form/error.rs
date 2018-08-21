use http::RawStr;

/// Error returned by the [`FromForm`] derive on form parsing errors.
///
/// If multiple errors occur while parsing a form, the first error in the
/// following precedence, from highest to lowest, is returned:
///
///   * `BadValue` or `Unknown` in incoming form string field order
///   * `Missing` in lexical field order
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum FormError<'f> {
    /// The field named `.0` with value `.1` failed to parse or validate.
    BadValue(&'f RawStr, &'f RawStr),
    /// The parse was strict and the field named `.0` with value `.1` appeared
    /// in the incoming form string but was unexpected.
    ///
    /// This error cannot occur when parsing is lenient.
    Unknown(&'f RawStr, &'f RawStr),
    /// The field named `.0` was expected but is missing in the incoming form.
    Missing(&'f RawStr),
}
