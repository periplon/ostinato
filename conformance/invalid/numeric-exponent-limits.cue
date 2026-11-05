package invalidexponents

import "math"

add:  1e100000 + 1e-100000
div:  1e-100000 / 3
mul:  1e99999 * 100
exp:  math.Exp(23000)
pow:  math.Pow(10, 100001)
cbrt: math.Cbrt(1e-100000)
