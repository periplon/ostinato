package timelayouts

import "time"

params: {
	ansic:        time.Parse(time.ANSIC, "Tue Mar  5 06:07:08 2024")
	unix_date:    time.Parse(time.UnixDate, "Tue Mar  5 06:07:08 UTC 2024")
	ruby_date:    time.Parse(time.RubyDate, "Tue Mar 05 06:07:08 +0200 2024")
	rfc822:       time.Parse(time.RFC822, "05 Mar 24 06:07 UTC")
	rfc850:       time.Parse(time.RFC850, "Tuesday, 05-Mar-24 06:07:08 UTC")
	rfc1123:      time.Parse(time.RFC1123, "Tue, 05 Mar 2024 06:07:08 UTC")
	rfc1123z:     time.Parse(time.RFC1123Z, "Tue, 05 Mar 2024 06:07:08 +0200")
	rfc3339:      time.Parse(time.RFC3339, "2024-03-05T06:07:08+02:00")
	rfc3339_nano: time.Parse(time.RFC3339Nano, "2024-03-05T06:07:08.123456789+02:00")
	date:         time.Parse(time.RFC3339Date, "2024-03-05")
	kitchen:      time.Parse(time.Kitchen, "3:04PM")
	kitchen24:    time.Parse(time.Kitchen24, "06:07")
	custom:       time.Parse("Mon 2 Jan 2006", "Tue 5 Mar 2024")
	year_day:     time.Parse("2006-002", "2024-065")
	format:       time.Format("2024-03-05", "2006-01-02")
	formatted:    time.FormatString("Mon Jan _2 15:04:05 2006", "2024-03-05T06:07:08Z")
	to_unix:      time.ToUnix("2024-03-05T06:07:08Z")
	to_unix_nano: time.ToUnixNano("2024-03-05T06:07:08.000000005Z")
	split:        time.Split("2024-03-05T06:07:08.12Z")
	duration:     time.FormatDuration(3723000500000)
	second:       time.Second
	fraction_exact:      time.FormatString("15:04:05.000", "2024-03-05T06:07:08.123456789Z")
	fraction_exact_zero: time.FormatString("15:04:05.000", "2024-03-05T06:07:08Z")
	fraction_trim:       time.FormatString("15:04:05.999", "2024-03-05T06:07:08.123456789Z")
	fraction_comma:      time.FormatString("15:04:05,000", "2024-03-05T06:07:08.123456789Z")
	fraction_long:       time.Parse(time.RFC3339Nano, "2024-03-05T06:07:08.123456789012Z")
	zone_seconds:        time.Parse("2006-01-02T15:04:05Z070000", "2024-03-05T06:07:08+020030")
	zone_gmt:            time.Parse("MST", "GMT+2")
	zone_aest:           time.Parse("MST", "AEST")
	space_run:           time.Parse("2006-01-02 15:04", "2024-03-05  06:07")
	hour_short:          time.Parse("15:04", "2:04")
	pm_no_hour:          time.Parse("15:04PM", "15:04PM")
	zone_24:             time.FormatString("15:04 -07:00", "2024-03-05T06:07:08+24:00")
	yday_month:          time.Parse("2006-002 01", "2024-065 03")
	yday_day:            time.Parse("2006-002 02", "2024-065 05")
	literal_month:       time.FormatString("Januar", "2024-03-05T06:07:08Z")
	literal_weekday:     time.FormatString("Monat 2006", "2024-03-05T06:07:08Z")
	underscore_year:     time.FormatString("_2006", "2024-03-05T06:07:08Z")
	underscore_parse:    time.Parse("_2006", "_2024")
	trailing_space:      time.Parse("2006-01-02 ", "2024-03-05")
	utc_shortcut:        time.Parse("MSTX", "UTCX")
	duration_round:      time.ParseDuration("1.999999999999999999999s")
	duration_long:       time.ParseDuration("0." + ("9"*100) + "s")
	fraction_literal:    time.Parse(".999.", ".")
	seconds_literal:     time.Parse("15:04:05.", "12:30:00.")
}

workflow: {
	name: "time-layouts"
	steps: {s: {kind: "noop"}}
}
