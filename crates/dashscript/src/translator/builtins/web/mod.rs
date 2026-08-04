//! WinterTC Web API mappings — host-defined Web globals (Ecma TC55, ex-WinterCG
//! «Minimum Common Web API»): `console`, the WHATWG Encoding API
//! (`TextEncoder`/`TextDecoder`), and forthcoming `URL`/`crypto`/`atob`/….
//! These are *not* ES built-ins (they live outside ECMA-262) and *not* Node
//! modules (they are globals, not `node:` imports) — they are the Web platform
//! surface WinterTC standardises, mapped to Rust crates (`url`/`uuid`/`base64`/
//! …) the way Deno's `ext/web` + `ext/<api>/` modules back the same APIs.
//!
//! Second layer of `builtins/` — ES built-in / **Web API** / Node module / test
//! harness. Today only `console` and `encoding` are mapped (migrated from the
//! ES-built-in top level where they were historical outliers); the remaining
//! WinterTC surface lands here as Tier-1 static mappings (see the WinterTC
//! roadmap, task #408).

mod blob;
mod compression;
mod console;
mod crypto;
mod encoding;
mod eventtarget;
mod file;
mod form_data;
mod headers;
mod hr_time;
mod streams;
mod timers;
mod url;
mod urlpattern;

pub(in crate::translator) use blob::{blob_ctor, blob_ctor_type, blob_method};
pub(in crate::translator) use compression::{
    compression_ctor_type, compression_method, compression_stream_ctor,
};
pub(in crate::translator) use console::console_method;
pub(in crate::translator) use crypto::crypto_method;
pub(in crate::translator) use encoding::{
    encoding_ctor_type, text_decoder_method, text_encoder_method,
};
pub(in crate::translator) use eventtarget::{
    abort_method, event_init, event_target_ctor_type, event_target_method,
};
pub(in crate::translator) use file::{file_ctor, file_ctor_type};
pub(in crate::translator) use form_data::{form_data_ctor, form_data_ctor_type, form_data_method};
pub(in crate::translator) use headers::{headers_ctor, headers_ctor_type, headers_method};
pub(in crate::translator) use hr_time::perf_method;
pub(in crate::translator) use streams::{readable_stream_ctor, streams_ctor_type, streams_method};
pub(in crate::translator) use timers::timer_function;
pub(in crate::translator) use url::{
    url_ctor_type, url_search_params_method, url_search_params_on_url_method, url_static_method,
};
pub(in crate::translator) use urlpattern::urlpattern_ctor_type;
