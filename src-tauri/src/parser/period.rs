use crate::models::NoteKind;
use chrono::NaiveDate;

/// Recency window for harvesting open tasks from daily notes. Other calendar
/// kinds are bounded by the calendar itself and are always harvested in full.
pub const DAILY_WINDOW_DAYS: i64 = 30;

fn stem(relative_path: &str) -> Option<&str> {
    std::path::Path::new(relative_path).file_stem()?.to_str()
}

/// Display period string for a calendar note, per NotePlan naming:
/// daily `YYYYMMDD` -> `YYYY-MM-DD`; weekly/monthly/quarterly/yearly stems
/// are already display-shaped (`2026-W27`, `2026-07`, `2026-Q3`, `2026`).
/// Returns None for non-calendar kinds and unparseable daily stems.
pub fn calendar_period(kind: &NoteKind, relative_path: &str) -> Option<String> {
    let stem = stem(relative_path)?;
    match kind {
        NoteKind::Daily => {
            let d = NaiveDate::parse_from_str(stem, "%Y%m%d").ok()?;
            Some(d.format("%Y-%m-%d").to_string())
        }
        NoteKind::Weekly | NoteKind::Monthly | NoteKind::Quarterly | NoteKind::Yearly => {
            Some(stem.to_string())
        }
        NoteKind::Regular | NoteKind::Template => None,
    }
}

/// Whether a daily note falls inside the harvest window ending at `today`.
/// Future-dated dailies are always in-window (they're planned, not stale).
/// Unparseable stems are out-of-window (they only appear with include-older).
pub fn daily_within_window(relative_path: &str, today: NaiveDate) -> bool {
    let Some(stem) = stem(relative_path) else {
        return false;
    };
    let Ok(d) = NaiveDate::parse_from_str(stem, "%Y%m%d") else {
        return false;
    };
    today.signed_duration_since(d).num_days() <= DAILY_WINDOW_DAYS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn test_calendar_period_formats() {
        assert_eq!(
            calendar_period(&NoteKind::Daily, "Calendar/20260702.md").as_deref(),
            Some("2026-07-02")
        );
        assert_eq!(
            calendar_period(&NoteKind::Weekly, "Calendar/2026-W27.md").as_deref(),
            Some("2026-W27")
        );
        assert_eq!(
            calendar_period(&NoteKind::Monthly, "Calendar/2026-07.md").as_deref(),
            Some("2026-07")
        );
        assert_eq!(
            calendar_period(&NoteKind::Quarterly, "Calendar/2026-Q3.md").as_deref(),
            Some("2026-Q3")
        );
        assert_eq!(
            calendar_period(&NoteKind::Yearly, "Calendar/2026.md").as_deref(),
            Some("2026")
        );
        assert_eq!(calendar_period(&NoteKind::Regular, "Notes/x.md"), None);
        assert_eq!(
            calendar_period(&NoteKind::Daily, "Calendar/garbage.md"),
            None
        );
    }

    #[test]
    fn test_daily_window() {
        let today = day(2026, 7, 5);
        assert!(daily_within_window("Calendar/20260705.md", today));
        assert!(daily_within_window("Calendar/20260605.md", today)); // exactly 30 days
        assert!(!daily_within_window("Calendar/20260604.md", today)); // 31 days
        assert!(daily_within_window("Calendar/20260801.md", today)); // future: in-window
        assert!(!daily_within_window("Calendar/20240101.md", today));
        assert!(!daily_within_window("Calendar/garbage.md", today));
    }
}
