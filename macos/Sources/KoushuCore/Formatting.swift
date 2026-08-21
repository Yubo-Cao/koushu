import Foundation

/// Number and byte formatting shared by the windows.
public enum Format {
    /// Download sizes, at the precision each magnitude deserves.
    ///
    /// Not `ByteCountFormatter`: it uses SI units by default and localises the
    /// unit names, and a model listed as "0.94 GB" here and "897 MB" on the
    /// download page it came from reads as two different files.
    public static func bytes(_ value: Int64?) -> String {
        guard let value, value > 0 else { return "–" }
        let kilobyte: Int64 = 1024
        let megabyte = kilobyte * 1024
        let gigabyte = megabyte * 1024
        if value >= gigabyte {
            return String(format: "%.1f GB", Double(value) / Double(gigabyte))
        }
        if value >= megabyte {
            return String(format: "%.0f MB", Double(value) / Double(megabyte))
        }
        return String(format: "%.0f KB", Double(value) / Double(kilobyte))
    }

    public static func downloadProgress(downloaded: Int64, total: Int64?, _ l: Localizer) -> String {
        if let total, total > 0, downloaded > 0 {
            return "\(bytes(downloaded)) / \(bytes(total))"
        }
        if downloaded > 0 {
            return l(.downloadDownloaded(size: bytes(downloaded)))
        }
        return l(.downloadPreparing)
    }

    /// `-90 dB` when the meter has nothing to report, rather than `-inf`.
    public static func decibels(_ value: Double) -> String {
        guard value.isFinite else { return "-90 dB" }
        return "\(Int(value.rounded())) dB"
    }

    /// The day heading a group of sessions sits under.
    ///
    /// Takes the stored `YYYY-MM-DD` key rather than a `Date` so the heading and
    /// the grouping can never disagree about which day a session is on.
    public static func dayHeading(dateKey: String, locale: UILocale) -> String {
        let parser = DateFormatter()
        parser.dateFormat = "yyyy-MM-dd"
        parser.locale = Foundation.Locale(identifier: "en_US_POSIX")
        guard let date = parser.date(from: dateKey) else { return dateKey }

        let calendar = Calendar.current
        if calendar.isDateInToday(date) {
            return locale == .zh ? "今天" : "Today"
        }
        if calendar.isDateInYesterday(date) {
            return locale == .zh ? "昨天" : "Yesterday"
        }
        let display = DateFormatter()
        display.locale = Foundation.Locale(identifier: locale == .zh ? "zh_Hans_CN" : "en_US")
        display.setLocalizedDateFormatFromTemplate(
            calendar.isDate(date, equalTo: Date(), toGranularity: .year) ? "MMMd" : "yMMMd"
        )
        return display.string(from: date)
    }

    public static func time(_ date: Date, locale: UILocale) -> String {
        let formatter = DateFormatter()
        formatter.locale = Foundation.Locale(identifier: locale == .zh ? "zh_Hans_CN" : "en_US")
        formatter.setLocalizedDateFormatFromTemplate("jm")
        return formatter.string(from: date)
    }

    /// `YYYY-MM-DD`, the form the date filters and `dateKey` use.
    public static func dateKey(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd"
        formatter.locale = Foundation.Locale(identifier: "en_US_POSIX")
        return formatter.string(from: date)
    }

    public static func date(fromKey key: String) -> Date? {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd"
        formatter.locale = Foundation.Locale(identifier: "en_US_POSIX")
        return formatter.date(from: key)
    }

    public static func elapsed(_ seconds: TimeInterval) -> String {
        let whole = max(0, Int(seconds))
        return String(format: "%d:%02d", whole / 60, whole % 60)
    }
}

/// Splits `text` so each occurrence of any search term can be marked.
///
/// Done on the rendered snippet rather than by returning offsets from the core:
/// a Rust `char` index and a Swift `String.Index` disagree the moment an emoji
/// appears, and a highlight one character off is worse than none.
public func highlightRanges(in text: String, terms: [String]) -> [Range<String.Index>] {
    let usable = terms.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
    guard !usable.isEmpty else { return [] }

    var ranges: [Range<String.Index>] = []
    // Longest first, so "transcription" is marked whole rather than as
    // "transcript" plus a stray tail.
    for term in usable.sorted(by: { $0.count > $1.count }) {
        var cursor = text.startIndex
        while cursor < text.endIndex,
              let found = text.range(of: term, options: [.caseInsensitive, .diacriticInsensitive], range: cursor..<text.endIndex) {
            if !ranges.contains(where: { $0.overlaps(found) }) {
                ranges.append(found)
            }
            cursor = found.upperBound
        }
    }
    return ranges.sorted { $0.lowerBound < $1.lowerBound }
}
