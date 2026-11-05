// axdrive: a tiny Accessibility-API driver for the disposable test VM only. It needs no AppleEvents (so no
// "wants to control System Events" prompt): only kTCCServiceAccessibility (and Screen Recording for window titles),
// which the cirruslabs base image grants to tart-guest-agent, the responsible process of everything `tart exec` starts.
//
//   axdrive windows                      on-screen windows (owner, pid, layer, title, bounds) from CGWindowList, as TSV
//   axdrive tree <app> [depth]           the AX tree of an app (name or bundle id or pid), for finding elements
//   axdrive press <app> <regex> [role] [n]   AXPress the first element (default role AXButton) whose title/description matches
//   axdrive has <app> <regex> [role]     exit 0 if such an element exists (prints its path), else 1
//   axdrive trusted                      exit 0 if this process is AX-trusted
// Build (host): swiftc -O -o axdrive axdrive.swift ; never run it on the host.
import AppKit
import ApplicationServices
import Foundation

setvbuf(stdout, nil, _IOLBF, 0)
let args = CommandLine.arguments
func die(_ m: String, _ code: Int32 = 2) -> Never { FileHandle.standardError.write((m + "\n").data(using: .utf8)!); exit(code) }

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
  var v: AnyObject?
  return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}
func str(_ e: AXUIElement, _ name: String) -> String { (attr(e, name) as? String) ?? "" }
func children(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }
func label(_ e: AXUIElement) -> String {
  var parts = [str(e, kAXTitleAttribute), str(e, kAXDescriptionAttribute), str(e, kAXValueAttribute), str(e, kAXIdentifierAttribute)]
  if let n = attr(e, kAXValueAttribute) as? NSNumber { parts.append("value=\(n)") }
  return parts.filter { !$0.isEmpty }.joined(separator: " | ")
}

/// The texts a regex is tried against: the whole label, and the title and the description on their own. A system sheet's button has
/// the label "Always Allow | _NS:21" (title plus identifier), so `^Always Allow$` never matched the joined label although the button was
/// on screen (2026-11-05); anchoring on the title alone is what the callers mean.
func candidates(_ e: AXUIElement) -> [String] {
  [label(e), str(e, kAXTitleAttribute), str(e, kAXDescriptionAttribute)].filter { !$0.isEmpty }
}
func matches(_ re: NSRegularExpression, _ e: AXUIElement) -> Bool {
  candidates(e).contains { re.firstMatch(in: $0, range: NSRange($0.startIndex..., in: $0)) != nil }
}

func pidFor(_ app: String) -> pid_t? {
  if let p = Int32(app) { return p }
  let apps = NSWorkspace.shared.runningApplications
  if let a = apps.first(where: { $0.bundleIdentifier == app || $0.localizedName == app }) { return a.processIdentifier }
  // Agents such as CoreServicesUIAgent or UserNotificationCenter: fall back to the process name.
  let task = Process(); task.launchPath = "/usr/bin/pgrep"; task.arguments = ["-x", app]
  let pipe = Pipe(); task.standardOutput = pipe; try? task.run(); task.waitUntilExit()
  let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
  return out.split(separator: "\n").compactMap { Int32($0) }.first
}

func walk(_ e: AXUIElement, depth: Int, maxDepth: Int, path: String, visit: (AXUIElement, String, Int) -> Bool) -> Bool {
  if visit(e, path, depth) { return true }
  if depth >= maxDepth { return false }
  for (i, c) in children(e).enumerated() {
    if walk(c, depth: depth + 1, maxDepth: maxDepth, path: "\(path)/\(str(c, kAXRoleAttribute))[\(i)]", visit: visit) { return true }
  }
  return false
}

guard args.count >= 2 else { die("usage: axdrive windows|tree|press|has|trusted ...") }
switch args[1] {
case "trusted":
  exit(AXIsProcessTrusted() ? 0 : 1)
case "windows":
  let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]]) ?? []
  for w in list {
    let b = (w[kCGWindowBounds as String] as? [String: Any]) ?? [:]
    print([w[kCGWindowOwnerName as String] as? String ?? "", "\(w[kCGWindowOwnerPID as String] ?? "")",
           "\(w[kCGWindowLayer as String] ?? "")", w[kCGWindowName as String] as? String ?? "",
           "\(b["X"] ?? 0),\(b["Y"] ?? 0),\(b["Width"] ?? 0)x\(b["Height"] ?? 0)"].joined(separator: "\t"))
  }
case "tree", "press", "has":
  guard args.count >= 3, let pid = pidFor(args[2]) else { die("no running app \(args.count >= 3 ? args[2] : "")", 3) }
  guard AXIsProcessTrusted() else { die("not AX-trusted (grant Accessibility to the responsible process)", 4) }
  let root = AXUIElementCreateApplication(pid)
  if args[1] == "tree" {
    let maxDepth = args.count >= 4 ? Int(args[3]) ?? 12 : 12
    _ = walk(root, depth: 0, maxDepth: maxDepth, path: "") { e, _, d in
      print(String(repeating: "  ", count: d) + str(e, kAXRoleAttribute) + "  " + label(e)); return false
    }
  } else {
    guard args.count >= 4, let re = try? NSRegularExpression(pattern: args[3]) else { die("need a regex") }
    let role = args.count >= 5 ? args[4] : kAXButtonRole
    var found: AXUIElement?; var where_ = ""
    var skip = args.count >= 6 ? Int(args[5]) ?? 0 : 0 // the n-th match, 0-based (a dashboard can hold two widgets with one name)
    _ = walk(root, depth: 0, maxDepth: 30, path: "") { e, p, _ in
      guard str(e, kAXRoleAttribute) == role else { return false }
      let l = label(e)
      if matches(re, e) { if skip == 0 { found = e; where_ = "\(p)  [\(l)]"; return true }; skip -= 1 }
      return false
    }
    guard let el = found else { print("not found"); exit(1) }
    print(where_)
    if args[1] == "press" {
      let r = AXUIElementPerformAction(el, kAXPressAction as CFString)
      guard r == .success else { die("AXPress failed: \(r.rawValue)", 5) }
      print("pressed")
    }
  }
case "setfield":
  // axdrive setfield <app> <n>: sets the n-th (0-based) AXTextField/AXSecureTextField of the app to a value read from stdin
  // (stdin, so a test password never appears in a process list).
  guard args.count >= 4, let pid = pidFor(args[2]), let n = Int(args[3]) else { die("usage: setfield <app> <n>  (value on stdin)") }
  let value = (readLine(strippingNewline: true) ?? "")
  var fields: [AXUIElement] = []
  _ = walk(AXUIElementCreateApplication(pid), depth: 0, maxDepth: 30, path: "") { e, _, _ in
    let r = str(e, kAXRoleAttribute); if r == kAXTextFieldRole || r == "AXSecureTextField" { fields.append(e) }; return false
  }
  guard n < fields.count else { die("only \(fields.count) text fields", 1) }
  AXUIElementSetAttributeValue(fields[n], kAXFocusedAttribute as CFString, kCFBooleanTrue)
  let r = AXUIElementSetAttributeValue(fields[n], kAXValueAttribute as CFString, value as CFString)
  print(r == .success ? "set" : "set failed: \(r.rawValue)"); exit(r == .success ? 0 : 5)
case "center":
  // axdrive center <app> <regex> <role> [n]: prints "x y" of the element's centre in screen points.
  guard args.count >= 5, let pid = pidFor(args[2]), let re = try? NSRegularExpression(pattern: args[3]) else { die("usage: center <app> <regex> <role>") }
  var found: AXUIElement?
  var skip = args.count >= 6 ? Int(args[5]) ?? 0 : 0 // the n-th match, 0-based (an open list repeats the text of the box that opened it)
  _ = walk(AXUIElementCreateApplication(pid), depth: 0, maxDepth: 30, path: "") { e, _, _ in
    guard str(e, kAXRoleAttribute) == args[4] else { return false }
    if matches(re, e) { if skip == 0 { found = e; return true }; skip -= 1 }; return false
  }
  guard let el = found, let pv = attr(el, kAXPositionAttribute), let sv = attr(el, kAXSizeAttribute) else { print("not found"); exit(1) }
  var p = CGPoint.zero, s = CGSize.zero
  AXValueGetValue(pv as! AXValue, .cgPoint, &p); AXValueGetValue(sv as! AXValue, .cgSize, &s)
  print("\(Int(p.x + s.width / 2)) \(Int(p.y + s.height / 2))")
case "drag":
  // axdrive drag x1 y1 x2 y2: a real left-button drag (mouse down, moves, up) through the HID event tap, like a hand.
  guard args.count >= 6, let x1 = Double(args[2]), let y1 = Double(args[3]), let x2 = Double(args[4]), let y2 = Double(args[5]) else { die("usage: drag x1 y1 x2 y2") }
  let src = CGEventSource(stateID: .hidSystemState)
  var last = CGPoint(x: x1, y: y1)
  func post(_ t: CGEventType, _ x: Double, _ y: Double) {
    guard let ev = CGEvent(mouseEventSource: src, mouseType: t, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left) else { return }
    ev.setIntegerValueField(.mouseEventClickState, value: 1)
    ev.setIntegerValueField(.mouseEventDeltaX, value: Int64(x - last.x)); ev.setIntegerValueField(.mouseEventDeltaY, value: Int64(y - last.y))
    ev.post(tap: .cghidEventTap); last = CGPoint(x: x, y: y)
  }
  post(.mouseMoved, x1, y1); usleep(300_000)
  post(.leftMouseDown, x1, y1); usleep(150_000)
  for i in 1...8 { post(.leftMouseDragged, x1 + Double(i), y1 + Double(i)); usleep(40_000) }  // past the drag threshold
  let sx = x1 + 8, sy = y1 + 8
  for i in 1...60 { let f = Double(i) / 60; post(.leftMouseDragged, sx + (x2 - sx) * f, sy + (y2 - sy) * f); usleep(40_000) }
  for _ in 1...10 { post(.leftMouseDragged, x2, y2); usleep(100_000) }  // hover over the target so it highlights
  post(.leftMouseUp, x2, y2)
  print("dragged")
case "type":
  // axdrive type: posts the stdin text as keyboard events to the focused element (CGEvent, needs PostEvent), then Return
  // if --return is given.
  let text = readLine(strippingNewline: true) ?? ""
  let src = CGEventSource(stateID: .hidSystemState)
  for u in text.utf16 {
    var c = u
    for down in [true, false] {
      let ev = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: down)
      ev?.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
      ev?.post(tap: .cghidEventTap); usleep(20_000)
    }
  }
  if args.contains("--return") {
    for down in [true, false] { CGEvent(keyboardEventSource: src, virtualKey: 36, keyDown: down)?.post(tap: .cghidEventTap); usleep(20_000) }
  }
  print("typed \(text.count) chars")
default:
  die("unknown command \(args[1])")
}
