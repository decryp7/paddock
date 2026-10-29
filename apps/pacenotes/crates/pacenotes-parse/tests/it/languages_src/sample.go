package sample

import (
	"fmt"
	"strings"
)

const Version = "1"

var counter int

type Shape interface {
	Area() float64
}

type Rect struct {
	W, H float64
	name string
}

type ID int

func NewRect(w, h float64) *Rect {
	fmt.Println("new")
	return &Rect{W: w, H: h}
}

func (r *Rect) Area() float64 {
	local := strings.ToUpper("x")
	_ = local
	return r.W * r.H
}
