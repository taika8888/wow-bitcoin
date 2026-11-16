use std::ffi::{CStr, CString};
use std::ptr;

use serde_json::{Value, json};
use ticker_ffi::{Ticker, ticker_create, ticker_destroy, ticker_handle, ticker_string_free, ticker_view};

struct Handle(*mut Ticker);
impl Handle {
    fn new(settings: &str) -> Self {
        let mut error = ptr::null_mut();
        let settings = CString::new(settings).unwrap();
        // SAFETY: buffers and output slot are live, and the handle is exclusively owned here.
        let app = unsafe { ticker_create(settings.as_ptr(), c"\"wow\"".as_ptr(), 1000.0, &mut error) };
        assert!(error.is_null());
        assert!(!app.is_null());
        Self(app)
    }
    fn send(&self, input: Value) -> Value {
        let input = CString::new(input.to_string()).unwrap();
        // SAFETY: handle and input remain live for this synchronous call.
        take(unsafe { ticker_handle(self.0, input.as_ptr()) })
    }
    fn view(&self, bars: usize) -> Value {
        // SAFETY: handle is live and never used concurrently.
        take(unsafe { ticker_view(self.0, bars, -18000) })
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper is the handle's sole owner.
        unsafe { ticker_destroy(self.0) };
    }
}
fn take(value: *mut std::ffi::c_char) -> Value {
    assert!(!value.is_null());
    // SAFETY: the library returns an owned NUL-terminated string, freed exactly once here.
    unsafe {
        let result = serde_json::from_slice(CStr::from_ptr(value).to_bytes()).unwrap();
        ticker_string_free(value);
        result
    }
}

#[test]
fn settings_round_trip_keeps_the_mac_json_shape_and_coordinates() {
    let app = Handle::new(r#"{"position":{"x":-120,"y":840},"onlyWhenWoWActive":true,"zoom":4,"decimals":2}"#);
    let settings = app.view(0)["settings"].clone();
    assert_eq!(settings["position"], json!({"x": -120.0, "y": 840.0}));
    assert_eq!(settings["onlyWhenWoWActive"], true);
    assert_eq!(settings["zoom"], 4);
    let copy = Handle::new(&settings.to_string());
    assert_eq!(copy.view(0)["settings"], settings);
}

#[test]
fn request_ids_effects_and_duplicate_results_cross_the_abi() {
    let app = Handle::new("{}");
    let first = app.send(json!({"type": "tick", "now": 1000}));
    assert_eq!(first["effects"][0]["type"], "fetch_quote");
    assert_eq!(first["effects"][0]["timeout"], 5.0);
    let result = json!({"type": "quote_finished", "request": first["effects"][0]["request"], "now": 1001,
        "outcome": {"type": "answered", "status": 200, "body": r#"{"result":{"XXBTZUSD":{"c":["80482.4"],"o":"80442"}}}"#}});
    assert_eq!(app.send(result.clone())["effects"], json!([{"type": "render"}]));
    let view = app.view(0);
    assert_eq!(view["status"], json!({"name": "live", "fetchedAt": 1001.0}));
    assert_eq!(view["display"]["segments"][0]["text"], "BTC $80,482");
    assert_eq!(app.send(result)["effects"], json!([]));
    assert_eq!(
        app.send(json!({"type": "overlay", "event": {"type": "toggle_lock"}}))["effects"][0]["type"],
        "save_settings"
    );
    assert_eq!(app.send(json!({"type": "settings_changed", "settings": {}}))["effects"], json!([{"type": "render"}]));
}

#[test]
fn chart_metadata_does_not_serialize_bars_on_quote_updates() {
    let app = Handle::new("{}");
    app.send(json!({"type":"overlay", "event":{"type":"pressed"}}));
    let reply = app.send(json!({"type":"overlay", "event":{"type":"released", "over":"body", "x":0, "y":0}}));
    let request = &reply["effects"][0]["request"];
    app.send(json!({"type":"candles_finished", "request":request, "timeframe":"m15", "now":1001,
        "outcome":{"type":"answered", "status":200, "body":r#"{"result":{"XXBTZUSD":[[1000,"100","120","90","110"]]}}"#}}));
    assert!(app.view(0)["chart"].is_null());
    let view = app.view(150);
    assert_eq!(view["chartRevision"], 1);
    assert_eq!(view["chart"]["candleCount"], 1);
    assert_eq!(view["chart"]["bars"][0]["time"], 1450.0);
    assert_eq!(view["chart"]["bars"][0]["tone"], "up");
    assert!(!view["chart"]["priceTicks"].as_array().unwrap().is_empty());
    app.send(json!({"type":"tick", "now":1002}));
    assert_eq!(app.view(0)["chartRevision"], 1);
}

#[test]
fn malformed_inputs_nulls_and_invalid_utf8_return_errors_without_poisoning_the_app() {
    let app = Handle::new("{}");
    for input in [
        json!(null),
        json!({"type":"tick","now":"tomorrow"}),
        json!({"type":"unknown"}),
        json!({"type":"tick","now":1,"extra":0}),
    ] {
        assert!(app.send(input)["error"].is_string());
    }
    // SAFETY: null is documented as an error, and invalid UTF-8 is still a live NUL-terminated buffer.
    unsafe {
        assert!(take(ticker_handle(app.0, ptr::null()))["error"].is_string());
        assert!(take(ticker_handle(app.0, [255u8, 0].as_ptr().cast()))["error"].is_string());
        assert!(take(ticker_view(ptr::null(), 0, 0))["error"].is_string());
        ticker_string_free(ptr::null_mut());
        ticker_destroy(ptr::null_mut());
    }
    assert_eq!(app.view(0)["status"]["name"], "loading");
    assert_eq!(app.send(json!({"type":"tick","now":1000}))["effects"][0]["type"], "fetch_quote");
}

#[test]
fn invalid_creation_arguments_return_an_owned_error() {
    for now in [f64::NAN, f64::INFINITY] {
        let mut error = ptr::null_mut();
        // SAFETY: both strings and output slot are live.
        assert!(unsafe { ticker_create(c"{}".as_ptr(), c"\"wow\"".as_ptr(), now, &mut error) }.is_null());
        assert!(take(error)["error"].is_string());
    }
    let mut error = ptr::null_mut();
    // SAFETY: both strings and output slot are live.
    assert!(unsafe { ticker_create(c"{}".as_ptr(), c"\"unknown\"".as_ptr(), 0.0, &mut error) }.is_null());
    assert!(take(error)["error"].is_string());
}
