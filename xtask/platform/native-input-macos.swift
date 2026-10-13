import AppKit
import Carbon
import CoreGraphics
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}
guard CommandLine.arguments.count == 3, let ownedPID = Int32(CommandLine.arguments[1]) else {
    fail("Expected an owned PID and input operation")
}
let operation = CommandLine.arguments[2]
if operation == "ime-session" {
    do { try requireDisposableIME() }
    catch { fail(String(describing: error)) }
}
let pasteboard = NSPasteboard.general
if operation == "clipboard-read" {
    FileHandle.standardOutput.write(Data((pasteboard.string(forType: .string) ?? "").utf8))
    exit(0)
}
if operation == "clipboard-write" {
    guard let value = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8) else { fail("Clipboard input is not UTF-8") }
    pasteboard.clearContents()
    guard pasteboard.setString(value, forType: .string) else { fail("Cannot set the OS clipboard") }
    exit(0)
}
guard CGPreflightPostEventAccess() else {
    fail("macOS does not grant this helper Accessibility event-posting permission (TCC). No keyboard events were injected; this is not a platform input pass.")
}
guard let application = NSRunningApplication(processIdentifier: ownedPID) else { fail("Owned native process is no longer running") }
application.activate(options: [.activateIgnoringOtherApps])
let deadline = Date().addingTimeInterval(5)
while NSWorkspace.shared.frontmostApplication?.processIdentifier != ownedPID && Date() < deadline {
    Thread.sleep(forTimeInterval: 0.05)
}
guard NSWorkspace.shared.frontmostApplication?.processIdentifier == ownedPID else { fail("Cannot focus the owned native process") }
if operation == "focus" { exit(0) }
if operation == "ime-session" {
    do { try runIMESession(ownedPID: ownedPID, application: application) }
    catch { fail("Owned Bevy IME session failed: \(error)") }
    exit(0)
}
let key: CGKeyCode
let command: Bool
switch operation {
case "select-all": key = 0; command = true
case "copy": key = 8; command = true
case "paste": key = 9; command = true
case "right": key = 124; command = false
case "backspace": key = 51; command = false
default: fail("Unknown input operation \(operation)")
}
guard let source = CGEventSource(stateID: .combinedSessionState) else { fail("Cannot create OS event source") }
func send(_ key: CGKeyCode, _ down: Bool, _ flags: CGEventFlags) {
    guard !application.isTerminated && NSWorkspace.shared.frontmostApplication?.processIdentifier == ownedPID else {
        fail("Owned native process lost focus before a key event")
    }
    guard let event = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down) else { fail("Cannot create OS key event") }
    event.flags = flags
    event.postToPid(ownedPID)
    Thread.sleep(forTimeInterval: 0.025)
}
if command { send(55, true, .maskCommand) }
send(key, true, command ? .maskCommand : [])
send(key, false, command ? .maskCommand : [])
if command { send(55, false, []) }

// Only the explicit disposable macOS IME fixture uses this persistent helper.
// Source ownership spans all real-key operations and is restored on stdin EOF,
// explicit finish, or a thrown failure. Ordinary keyboard/clipboard tests retain
// their existing short-lived helper path above.
struct IMEFailure: Error, CustomStringConvertible { let description: String }
func imeRequire(_ value: Bool, _ message: String) throws {
    if !value { throw IMEFailure(description: message) }
}
func imeProperty(_ source: TISInputSource, _ key: CFString) -> AnyObject? {
    guard let pointer = TISGetInputSourceProperty(source, key) else { return nil }
    return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue()
}
func imeSourceID(_ source: TISInputSource) -> String {
    imeProperty(source, kTISPropertyInputSourceID) as? String ?? ""
}
func imeSources(_ all: Bool = true) -> [TISInputSource] {
    if all { return TISCreateInputSourceList(nil, true).takeRetainedValue() as NSArray as! [TISInputSource] }
    return TISCreateInputSourceList(nil, false).takeRetainedValue() as NSArray as! [TISInputSource]
}
func imeEnabled() -> Set<String> {
    Set(imeSources().filter { imeProperty($0, kTISPropertyInputSourceIsEnabled) as? Bool == true }.map(imeSourceID))
}
func requireDisposableIME() throws {
    let env = ProcessInfo.processInfo.environment
    try imeRequire(env["LIMO_CAD_NATIVE_IME_TEST"] == "macos-japanese" &&
        env["GITHUB_ACTIONS"] == "true" && env["RUNNER_OS"] == "macOS" &&
        env["RUNNER_ENVIRONMENT"] == "github-hosted" && env["GITHUB_REPOSITORY_ID"] == "1313334315" &&
        env["GITHUB_RUN_ID"]?.range(of: "^[0-9]+$", options: .regularExpression) != nil,
        "Disposable GitHub macOS IME runner required")
}
func runIMESession(ownedPID: Int32, application: NSRunningApplication) throws {
    let env = ProcessInfo.processInfo.environment
    try requireDisposableIME()
    guard let temp = env["RUNNER_TEMP"], let outPath = env["LIMO_CAD_IME_OUT"],
          let hostPath = env["LIMO_CAD_IME_HOST_PATH"], let sessionID = env["LIMO_CAD_IME_SESSION"],
          let fieldID = env["LIMO_CAD_IME_FIELD_TOKEN"], !sessionID.isEmpty, !fieldID.isEmpty else {
        throw IMEFailure(description: "Missing exact owned fixture paths/session/field")
    }
    let root = URL(fileURLWithPath: temp).resolvingSymlinksInPath().standardizedFileURL.path + "/"
    let out = URL(fileURLWithPath: outPath).resolvingSymlinksInPath().standardizedFileURL
    let host = URL(fileURLWithPath: hostPath).resolvingSymlinksInPath().standardizedFileURL
    try imeRequire(outPath.hasPrefix("/") && out.path.hasPrefix(root), "IME evidence must be beneath RUNNER_TEMP")
    func ownedWindows() -> [[String: Any]] {
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        return windows.filter {
            ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == ownedPID &&
            ($0[kCGWindowLayer as String] as? NSNumber)?.intValue == 0
        }
    }
    let windows = ownedWindows()
    try imeRequire(windows.count == 1, "Expected exactly one visible normal window for the owned host")
    guard let windowID = windows[0][kCGWindowNumber as String] as? NSNumber else {
        throw IMEFailure(description: "Owned window has no OS identity")
    }
    func focus() throws {
        try imeRequire(CGPreflightPostEventAccess() && !application.isTerminated &&
            application.processIdentifier == ownedPID && ownedPID > 0 &&
            application.executableURL?.resolvingSymlinksInPath().standardizedFileURL == host &&
            NSWorkspace.shared.frontmostApplication?.processIdentifier == ownedPID &&
            ownedWindows().contains { ($0[kCGWindowNumber as String] as? NSNumber) == windowID },
            "Owned executable/window/focus or event-posting permission changed")
    }
    try focus()
    let targetID = "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese"
    let parentID = "com.apple.inputmethod.Kotoeri.RomajiTyping"
    let prior = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
    let initiallyEnabled = imeEnabled()
    var report: [String: Any] = ["status":"started", "owned_pid":ownedPID,
        "owned_executable":host.path, "window_number":windowID, "session":sessionID,
        "field_token":fieldID, "source_id":targetID, "prior_source":imeSourceID(prior),
        "initially_enabled":initiallyEnabled.sorted(), "os":ProcessInfo.processInfo.operatingSystemVersionString,
        "image_os":env["ImageOS"] ?? "", "image_version":env["ImageVersion"] ?? "",
        "run_id":env["GITHUB_RUN_ID"] ?? "", "sha":env["GITHUB_SHA"] ?? ""]
    var posted: [[String: Any]] = [], selected = false, finished = false
    var failure: Error?
    func save() throws {
        report["posted_keys"] = posted
        let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        try data.write(to: out.appendingPathComponent("macos-ime-driver.json"), options: .atomic)
    }
    func reply(_ value: [String: Any]) throws {
        var data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]); data.append(10)
        try FileHandle.standardOutput.write(contentsOf: data)
    }
    guard let eventSource = CGEventSource(stateID: .combinedSessionState) else {
        throw IMEFailure(description: "Cannot create OS key source")
    }
    func sendKey(_ key: CGKeyCode, _ down: Bool, _ flags: CGEventFlags) throws {
        try focus()
        try imeRequire(posted.count < 64 && imeSourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue()) == targetID,
                       "Japanese source changed or key budget exceeded")
        guard let event = CGEvent(keyboardEventSource: eventSource, virtualKey: key, keyDown: down) else {
            throw IMEFailure(description: "Cannot create real key event")
        }
        event.flags = flags; event.postToPid(ownedPID)
        posted.append(["sequence":posted.count + 1, "keycode":key, "down":down,
            "flags":flags.rawValue, "pid":ownedPID, "window_number":windowID,
            "source_id":targetID, "unix_ms":Date().timeIntervalSince1970 * 1000])
        try save(); Thread.sleep(forTimeInterval: 0.025)
    }
    do {
        try save(); try reply(["status":"ready", "window_number":windowID, "source_id":targetID])
        var sequence = 0, preedits = 0, commits = 0
        var collapsedEscapes = 0, selectedEscapes = 0
        while let line = readLine() {
            try imeRequire(line.utf8.count <= 4096, "IME request exceeds byte budget")
            guard let request = try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any],
                  let operation = request["operation"] as? String else {
                throw IMEFailure(description: "Malformed IME request")
            }
            sequence += 1
            try imeRequire(sequence <= 12 && request["sequence"] as? Int == sequence,
                           "IME request sequence/budget changed")
            if operation == "finish" { finished = true; break }
            let age = Date().timeIntervalSince1970 * 1000 - (request["checked_unix_ms"] as? Double ?? 0)
            try imeRequire(request["session"] as? String == sessionID && request["field_token"] as? String == fieldID &&
                !(request["focused_control"] as? String ?? "").isEmpty &&
                age >= -100 && age < 2000, "Missing fresh owned field/session focus receipt")
            try focus()
            switch operation {
            case "enable":
                try imeRequire(!selected, "Japanese input already enabled by this fixture")
                let modes = imeSources().filter { imeSourceID($0) == targetID }
                try imeRequire(modes.count == 1 && imeProperty(modes[0], kTISPropertyBundleID) as? String == parentID,
                               "Expected the installed Apple Romaji/Hiragana source")
                var attempts: [[String: Any]] = []
                for id in [parentID, targetID] {
                    let matches = imeSources().filter { imeSourceID($0) == id }
                    try imeRequire(matches.count == 1, "Input source inventory changed: \(id)")
                    if imeProperty(matches[0], kTISPropertyInputSourceIsEnabled) as? Bool == true { continue }
                    try imeRequire(imeProperty(matches[0], kTISPropertyInputSourceIsEnableCapable) as? Bool == true,
                                   "Input source is not enableable: \(id)")
                    let status = TISEnableInputSource(matches[0]); attempts.append(["id":id, "status":status])
                    report["enable_attempts"] = attempts; try save()
                    try imeRequire(status == noErr && imeEnabled().contains(id), "Cannot enable installed source: \(id)")
                }
                let available = imeSources(false).filter { imeSourceID($0) == targetID }
                try imeRequire(available.count == 1, "Enabled Japanese source is unavailable")
                selected = true
                let status = TISSelectInputSource(available[0]); report["select_status"] = status; try save()
                let deadline = Date().addingTimeInterval(3)
                while imeSourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue()) != targetID && Date() < deadline {
                    Thread.sleep(forTimeInterval: 0.025)
                }
                try imeRequire(status == noErr && imeSourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue()) == targetID,
                               "Japanese selection was not acknowledged by the OS")
            case "preedit":
                let ready = (preedits == 0 && commits == 0) ||
                    (preedits == 1 && commits == 1) ||
                    (preedits == 2 && commits == 1 && collapsedEscapes >= 1) ||
                    (preedits == 3 && commits == 1 && selectedEscapes >= 1)
                try imeRequire(selected && ready, "Unexpected preedit request")
                preedits += 1
                for key in [CGKeyCode(4), 0, 15, 32] { try sendKey(key, true, []); try sendKey(key, false, []) }
                try sendKey(59, true, .maskControl)
                try sendKey(38, true, .maskControl); try sendKey(38, false, .maskControl)
                try sendKey(59, false, [])
            case "commit":
                try imeRequire((preedits == 1 && commits == 0) || (preedits == 4 && commits == 1),
                               "Unexpected commit request")
                commits += 1; try sendKey(36, true, []); try sendKey(36, false, [])
            case "escape":
                try imeRequire(commits == 1 && ((preedits == 2 && collapsedEscapes < 2) ||
                    (preedits == 3 && selectedEscapes < 2)), "Unexpected cancellation request")
                if preedits == 2 { collapsedEscapes += 1 } else { selectedEscapes += 1 }
                try sendKey(53, true, []); try sendKey(53, false, [])
            default: throw IMEFailure(description: "Unknown IME operation: \(operation)")
            }
            try reply(["status":"applied", "sequence":sequence, "operation":operation,
                "posted_key_count":posted.count, "selected_source":imeSourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue())])
        }
        try imeRequire(finished && selected && preedits == 4 && commits == 2 &&
            (1...2).contains(collapsedEscapes) && (1...2).contains(selectedEscapes),
            "IME driver did not complete commit, collapsed cancel, selected cancel, and identical replacement")
    } catch { failure = error }
    // Refresh after composition: Apple can lazily enable its Kana Palette.
    // Only this disposable runner is allowed to restore the observed delta.
    var cleanup: [String: Any] = [:], errors: [String] = []
    if selected {
        let status = TISSelectInputSource(prior)
        cleanup["restore_status"] = status
        cleanup["restored_source"] = imeSourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue())
        if status != noErr || cleanup["restored_source"] as? String != imeSourceID(prior) { errors.append("Prior source was not restored") }
    }
    var disabled: [[String: Any]] = []
    for source in imeSources().filter({ !initiallyEnabled.contains(imeSourceID($0)) &&
        imeProperty($0, kTISPropertyInputSourceIsEnabled) as? Bool == true }).sorted(by: { imeSourceID($0).count > imeSourceID($1).count }) {
        let status = TISDisableInputSource(source); disabled.append(["id":imeSourceID(source), "status":status])
        if status != noErr { errors.append("Cannot disable newly enabled source: \(imeSourceID(source))") }
    }
    let finalEnabled = imeEnabled()
    cleanup["disabled"] = disabled; cleanup["enabled_set_restored"] = finalEnabled == initiallyEnabled
    cleanup["unexpected_enabled"] = finalEnabled.subtracting(initiallyEnabled).sorted()
    cleanup["unexpected_disabled"] = initiallyEnabled.subtracting(finalEnabled).sorted()
    if finalEnabled != initiallyEnabled { errors.append("Enabled input-source set was not restored exactly") }
    cleanup["errors"] = errors; report["cleanup"] = cleanup
    if let error = failure { report["error"] = String(describing: error) }
    report["status"] = failure == nil && errors.isEmpty ? "passed" : "failed"
    try save(); try reply(["status":"finished", "cleanup":cleanup, "result":report["status"]!])
    if let error = failure { throw error }
    try imeRequire(errors.isEmpty, errors.joined(separator: "; "))
}
