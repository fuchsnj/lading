//! The `splunk_hec_gen` library
//!
//! This crate is intended to back the `splunk_hec_gen` executable and is
//! not considered useful otherwise.

pub use worker::Worker;
mod acknowledgements;
pub mod config;
mod worker;

const SPLUNK_HEC_ACKNOWLEDGEMENTS_PATH: &str = "/services/collector/ack";
const SPLUNK_HEC_JSON_PATH: &str = "/services/collector/event";
const SPLUNK_HEC_TEXT_PATH: &str = "/services/collector/raw";
const SPLUNK_HEC_CHANNEL_HEADER: &str = "x-splunk-request-channel";
