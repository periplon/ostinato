package builtins

import (
	"math"
	"regexp"
	"time"
)

params: {
	rm: regexp.Match("^a", "abc")
	rf: regexp.Find("b+", "abbc")
	rfa: regexp.FindAll("b+", "abbcbb", -1)
	rr: regexp.ReplaceAll("b+", "abbc", "X")
	rq: regexp.QuoteMeta("a.b")

	ma: math.Abs(-2.5)
	mf: math.Floor(1.9)
	mc: math.Ceil(1.1)
	mr: math.Round(1.5)
	ms: math.Sqrt(4)
	mp: math.Pow(2, 10)
	mpi: math.Pi

	tu: time.Unix(1580000000, 0)
	tp: time.Parse("2006-01-02", "2020-01-02")
	tpd: time.ParseDuration("1h30m")
	tfrac: time.Parse("15:04:05", "12:30:00.5")
	tdur: time.ParseDuration("-1h30m")

	bytes1: 'abc'
	bytesLen: len(bytes1)
}

workflow: {
	name: "\(params.tfrac)-\(params.tdur)-\(params.rf)-\(params.rr)-\(params.rq)-\(params.ma)-\(params.mf)-\(params.ms)-\(params.tu)-\(params.tp)-\(params.bytesLen)"
	steps: {s: {kind: "noop"}}
}
