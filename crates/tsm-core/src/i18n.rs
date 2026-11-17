//! Language of user-facing messages produced by the engine (status text,
//! hints, validation problems). The desktop app sets it from the settings;
//! the default is English so logs/tests stay stable.

use std::sync::atomic::{AtomicBool, Ordering};

static THAI: AtomicBool = AtomicBool::new(false);

pub fn set_language(lang: &str) {
    THAI.store(lang.eq_ignore_ascii_case("th"), Ordering::Relaxed);
}

pub fn is_th() -> bool {
    THAI.load(Ordering::Relaxed)
}

/// `tr!("ข้อความไทย {x}", "English text {x}")` – both sides are `format!` strings.
#[macro_export]
macro_rules! tr {
    ($th:literal, $en:literal) => {
        if $crate::i18n::is_th() { format!($th) } else { format!($en) }
    };
    ($th:literal, $en:literal, $($arg:tt)*) => {
        if $crate::i18n::is_th() { format!($th, $($arg)*) } else { format!($en, $($arg)*) }
    };
}
