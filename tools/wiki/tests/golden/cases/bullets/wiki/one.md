---
title: T
verified: 2026-10-20
sources: [a.md]
---
Summary.

- Bare one.
- Linked [two](two.md).
* Star bare
* Star linked [two](two.md)
  - Nested bare
    continuation of nested [two](two.md)
- Wrapped
  continues
  here src: x
- By id src: `a`
- xsrc: not a word boundary
- _src: underscore before
- (src: paren before is a boundary)
- Empty link target [x]() here
- Empty link target but later [x]() and [y](two.md)
- Tab	bullet [t](two.md)
-	Tab after marker bare
-
- Only marker above
-nospace is not a bullet
  Indented text after nothing
```
- inside fence
```
  ```
- inside indented fence
  ```
- After fences bare
text line closes
- Final bare