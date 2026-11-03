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
}

workflow: {
	name: "time-layouts"
	steps: {s: {kind: "noop"}}
}
