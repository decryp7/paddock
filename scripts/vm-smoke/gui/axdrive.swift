// axdrive: the Accessibility-API driver of `scripts/vm-smoke --gui`, installed in the GUI test image only (gui-image.sh). Never
// run it on a host: it presses buttons in whatever session it runs in.
//
// It needs no AppleEvents (so no "wants to control System Events" prompt), only kTCCServiceAccessibility, which the cirruslabs
// base image grants to tart-guest-agent, the responsible process of everything `tart exec` starts.
//
//   axdrive trusted                       exit 0 if this process is AX-trusted
//   axdrive windows                       on-screen windows from CGWindowList, TSV: owner, pid, layer, title, x,y,wxh
//   axdrive tree <app> [depth]            the AX tree of an app (name, bundle id, process name or pid), one element per line
//   axdrive has <app> <regex> [role]      exit 0 if an element (default role AXButton) has a label matching the regex
//   axdrive press <app> <regex> [role]    AXPress that element
//   axdrive value <app> <regex> [role]    print that element's AXValue (a checkbox prints 0 or 1)
//   axdrive center <app> <regex> [role]   print "x y", the element's centre in screen points (for a VNC pointer)
//   axdrive setfield <app> <n>            set the n-th (0-based) text field to stdin's first line (a password never in argv)
//
// A label is title | description | value | identifier, joined with " | " (empty parts left out). Exit codes: 0 done, 1 not
// found, 2 usage, 3 no such app, 4 not AX-trusted, 5 the AX call failed.
import AppKit
import ApplicationServices
import Foundation

setvbuf(stdout, nil, _IOLBF, 0)
let args = CommandLine.arguments
func die(_ m: String, _ code: Int32) -> Never {
  FileHandle.standardError.write((m + "\n").data(using: .utf8)!)
  exit(code)
}

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

func pidFor(_ app: String) -> pid_t? {
  if let p = Int32(app) { return p }
  if let a = NSWorkspace.shared.runningApplications.first(where: { $0.bundleIdentifier == app || $0.localizedName == app }) {
    return a.processIdentifier
  }
  // Agents such as CoreServicesUIAgent, UserNotificationCenter or coreautha: by process name.
  let task = Process()
  task.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
  task.arguments = ["-x", app]
  let pipe = Pipe()
  task.standardOutput = pipe
  guard (try? task.run()) != nil else { return nil }
  task.waitUntilExit()
  let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
  return out.split(separator: "\n").compactMap { Int32($0) }.first
}

/// Depth-first walk; `visit` returns true to stop. Bounded by depth so a huge web view cannot run away.
@discardableResult
func walk(_ e: AXUIElement, depth: Int, maxDepth: Int, visit: (AXUIElement, Int) -> Bool) -> Bool {
  if visit(e, depth) { return true }
  if depth >= maxDepth { return false }
  for c in children(e) where walk(c, depth: depth + 1, maxDepth: maxDepth, visit: visit) { return true }
  return false
}

func root(_ app: String) -> AXUIElement {
  guard let pid = pidFor(app) else { die("no running app \(app)", 3) }
  guard AXIsProcessTrusted() else { die("not AX-trusted (grant Accessibility to the responsible process)", 4) }
  return AXUIElementCreateApplication(pid)
}

func find(_ app: String, _ pattern: String, _ role: String) -> AXUIElement? {
  guard let re = try? NSRegularExpression(pattern: pattern) else { die("bad regex \(pattern)", 2) }
  var found: AXUIElement?
  walk(root(app), depth: 0, maxDepth: 40) { e, _ in
    guard str(e, kAXRoleAttribute) == role else { return false }
    let l = label(e)
    if re.firstMatch(in: l, range: NSRange(l.startIndex..., in: l)) != nil { found = e; return true }
    return false
  }
  return found
}

guard args.count >= 2 else { die("usage: axdrive trusted|windows|tree|has|press|value|center|setfield ...", 2) }
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
case "tree":
  guard args.count >= 3 else { die("usage: tree <app> [depth]", 2) }
  let maxDepth = args.count >= 4 ? Int(args[3]) ?? 12 : 12
  walk(root(args[2]), depth: 0, maxDepth: maxDepth) { e, d in
    print(String(repeating: "  ", count: d) + str(e, kAXRoleAttribute) + "  " + label(e))
    return false
  }
case "has", "press", "value", "center":
  guard args.count >= 4 else { die("usage: \(args[1]) <app> <regex> [role]", 2) }
  let role = args.count >= 5 ? args[4] : kAXButtonRole
  guard let el = find(args[2], args[3], role) else { print("not found"); exit(1) }
  switch args[1] {
  case "press":
    let r = AXUIElementPerformAction(el, kAXPressAction as CFString)
    guard r == .success else { die("AXPress failed: \(r.rawValue)", 5) }
    print("pressed: \(label(el))")
  case "value":
    print((attr(el, kAXValueAttribute)).map { "\($0)" } ?? "")
  case "center":
    guard let pv = attr(el, kAXPositionAttribute), let sv = attr(el, kAXSizeAttribute) else { die("no position", 5) }
    var p = CGPoint.zero
    var s = CGSize.zero
    AXValueGetValue(pv as! AXValue, .cgPoint, &p)
    AXValueGetValue(sv as! AXValue, .cgSize, &s)
    print("\(Int(p.x + s.width / 2)) \(Int(p.y + s.height / 2))")
  default:
    print(label(el))
  }
case "setfield":
  guard args.count >= 4, let n = Int(args[3]) else { die("usage: setfield <app> <n>  (value on stdin)", 2) }
  let value = readLine(strippingNewline: true) ?? ""
  var fields: [AXUIElement] = []
  walk(root(args[2]), depth: 0, maxDepth: 40) { e, _ in
    let r = str(e, kAXRoleAttribute)
    if r == kAXTextFieldRole || r == "AXSecureTextField" { fields.append(e) }
    return false
  }
  guard n < fields.count else { die("only \(fields.count) text fields", 1) }
  AXUIElementSetAttributeValue(fields[n], kAXFocusedAttribute as CFString, kCFBooleanTrue)
  let r = AXUIElementSetAttributeValue(fields[n], kAXValueAttribute as CFString, value as CFString)
  guard r == .success else { die("setting the value failed: \(r.rawValue)", 5) }
  print("set")
default:
  die("unknown command \(args[1])", 2)
}
