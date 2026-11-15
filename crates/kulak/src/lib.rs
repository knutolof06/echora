//! Kulak: mikrofon yakalama ve konuşma tespiti.

/// Tüm zincirin ortak ses biçimi: 16 kHz, mono, 16 bit.
pub const ORNEK_HIZI: u32 = 16_000;
/// Bir parçanın süresi (ms) ve örnek sayısı.
pub const PARCA_MS: u32 = 30;
pub const PARCA_ORNEK: usize = (ORNEK_HIZI as usize * PARCA_MS as usize) / 1000;
pub mod vad;
pub mod yakalama;
