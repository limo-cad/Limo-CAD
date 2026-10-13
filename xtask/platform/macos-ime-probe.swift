// QA-only stock NSTextView. Observes real AppKit input; never injects text/IME callbacks.
import AppKit
import Carbon
import CoreGraphics
import Darwin
import Foundation

struct ProbeError: Error, CustomStringConvertible { let description: String }
func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw ProbeError(description: message) }
}
func property(_ source: TISInputSource, _ key: CFString) -> AnyObject? {
    guard let pointer = TISGetInputSourceProperty(source, key) else { return nil }
    return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue()
}
func sourceID(_ source: TISInputSource) -> String {
    property(source, kTISPropertyInputSourceID) as? String ?? ""
}
func sources(_ includeAll: Bool = true) -> [TISInputSource] {
    if includeAll {
        return TISCreateInputSourceList(nil, true).takeRetainedValue() as NSArray as! [TISInputSource]
    }
    return TISCreateInputSourceList(nil, false).takeRetainedValue() as NSArray as! [TISInputSource]
}
func describe(_ source: TISInputSource) -> [String: Any] {
    ["id": sourceID(source), "bundle": property(source, kTISPropertyBundleID) as? String ?? "",
     "name": property(source, kTISPropertyLocalizedName) as? String ?? "",
     "type": property(source, kTISPropertyInputSourceType) as? String ?? "",
     "mode": property(source, kTISPropertyInputModeID) as? String ?? "",
     "languages": property(source, kTISPropertyInputSourceLanguages) as? [String] ?? [],
     "enabled": property(source, kTISPropertyInputSourceIsEnabled) as? Bool ?? false,
     "enableable": property(source, kTISPropertyInputSourceIsEnableCapable) as? Bool ?? false,
     "selectable": property(source, kTISPropertyInputSourceIsSelectCapable) as? Bool ?? false]
}
func rangeJSON(_ range: NSRange) -> [String: Int] { ["location": range.location, "length": range.length] }
func stringValue(_ value: Any) -> String {
    (value as? NSAttributedString)?.string ?? (value as? String) ?? String(describing: value)
}
func writeJSON(_ value: [String: Any], to url: URL) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: url, options: .atomic)
}
func readJSON(_ url: URL) throws -> [String: Any]? {
    guard FileManager.default.fileExists(atPath: url.path) else { return nil }
    let data = try Data(contentsOf: url)
    try require(data.count <= 2 * 1024 * 1024, "Probe JSON exceeded its evidence bound")
    guard let value = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
        throw ProbeError(description: "Expected a probe JSON object: \(url.lastPathComponent)")
    }
    return value
}

// LaunchServices supplies the activation context that a shell-launched AppKit
// executable lacks. Supervise only this fresh bundle; never activate by name.
func launchProbe(out: URL, arguments: [String], environment: [String: String]) throws {
    let bundleURL = out.appendingPathComponent("StockIMEProbe.app").resolvingSymlinksInPath()
    let executableURL = bundleURL.appendingPathComponent("Contents/MacOS/macos-ime-probe")
    let identifier = "org.nobscad.qa.StockIMEProbe.run" + environment["GITHUB_RUN_ID"]!
    try require(bundleURL.path.hasPrefix(out.path + "/") &&
                Bundle(url: bundleURL)?.bundleIdentifier == identifier,
                "Expected the owned probe app bundle beneath the evidence directory")
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.activates = true; configuration.createsNewApplicationInstance = true
    configuration.allowsRunningApplicationSubstitution = false
    configuration.addsToRecentItems = false; configuration.promptsUserIfNeeded = false
    configuration.arguments = arguments + ["--app-child"]
    // LaunchServices does not inherit a shell's CI guard variables automatically.
    // Forward only the guards/provenance, never the runner's whole environment.
    let names = ["GITHUB_ACTIONS", "RUNNER_OS", "RUNNER_ENVIRONMENT", "GITHUB_REPOSITORY", "GITHUB_REPOSITORY_ID",
                 "GITHUB_RUN_ID", "GITHUB_SHA", "RUNNER_TEMP", "ImageOS", "ImageVersion"]
    configuration.environment = environment.filter { names.contains($0.key) }
    configuration.environment["LIMO_CAD_IME_SUPERVISOR_PID"] = String(getpid())
    var launch: [String: Any] = ["schema_version": 1, "status": "launching",
        "method": "NSWorkspace.openApplication", "bundle_url": bundleURL.path,
        "bundle_id": identifier, "launcher_pid": getpid(), "requested_activation": true,
        "started_utc": ISO8601DateFormatter().string(from: Date()),
        "event_posting_allowed": CGPreflightPostEventAccess(), "accessibility_trusted": AXIsProcessTrusted()]
    func save() throws {
        try writeJSON(launch, to: out.appendingPathComponent("launch.json"))
    }
    try save()
    var application: NSRunningApplication?, launchError: String?, completed = false
    let started = ProcessInfo.processInfo.systemUptime
    do {
        // The trusted shell-launched driver posts real OS events. The app bundle
        // only receives input and does not need event-posting permission itself.
        try require(CGPreflightPostEventAccess(), "TCC denies supervisor event posting; no app launched or input sent")
        guard let eventSource = CGEventSource(stateID: .combinedSessionState) else {
            throw ProbeError(description: "Cannot create the supervisor's OS event source")
        }
        NSWorkspace.shared.openApplication(at: bundleURL, configuration: configuration) { app, error in
            // Workspace callbacks use a concurrent queue; supervisor state stays
            // on the main thread, including its deadline and PID ownership.
            DispatchQueue.main.async {
                application = app; launchError = error.map(String.init(describing:)); completed = true
            }
        }
        while !completed && ProcessInfo.processInfo.systemUptime - started < 20 {
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        }
        try require(completed, "LaunchServices did not complete within 20 seconds")
        guard let owned = application else {
            throw ProbeError(description: "LaunchServices failed: \(launchError ?? "no application returned")")
        }
        // NSRunningApplication clears its PID/URLs after exit. Capture the PID
        // while alive; never replace it with a PID claimed by the report file.
        let launchPID = owned.processIdentifier
        launch["returned_application"] = ["pid": owned.processIdentifier,
            "bundle_id": owned.bundleIdentifier ?? "", "bundle_url": owned.bundleURL?.path ?? "",
            "executable_url": owned.executableURL?.path ?? "", "terminated": owned.isTerminated]
        func childReport(requireReturnedPID: Bool = true) throws -> [String: Any]? {
            guard let report = try readJSON(out.appendingPathComponent("report.json")),
                  let childEnvironment = report["environment"] as? [String: Any],
                  let childApplication = report["application"] as? [String: Any],
                  let reportPID = childEnvironment["pid"] as? Int, reportPID > 0,
                  (!requireReturnedPID || (launchPID > 0 && reportPID == Int(launchPID))),
                  childEnvironment["run_id"] as? String == environment["GITHUB_RUN_ID"],
                  childEnvironment["sha"] as? String == environment["GITHUB_SHA"],
                  childApplication["bundle_id"] as? String == identifier,
                  childApplication["bundle_url"] as? String == bundleURL.path,
                  childApplication["executable_url"] as? String == executableURL.path else { return nil }
            return report
        }
        func identity() throws {
            try require(!owned.isTerminated && launchPID > 0 && launchPID != getpid() &&
                        owned.processIdentifier == launchPID &&
                        owned.bundleURL?.resolvingSymlinksInPath().path == bundleURL.path &&
                        owned.executableURL?.resolvingSymlinksInPath().path == executableURL.path &&
                        owned.bundleIdentifier == identifier, "LaunchServices returned a different or terminated process/bundle")
        }
        // A failed child can exit before the asynchronous launch callback. Keep
        // its actionable error instead of treating vanished identity fields as
        // an unrelated app. This path can only report failure, never send keys.
        if let report = try childReport(requireReturnedPID: !owned.isTerminated),
           report["child_exit_code"] as? Int == 1 {
            launch["child_status"] = report["status"]; launch["child_exit_code"] = 1
            let reportPID = (report["environment"] as? [String: Any])?["pid"] as? Int
            launch["failed_child_pid_verified"] = launchPID > 0 && reportPID == Int(launchPID)
            launch["failed_child_report_pid"] = reportPID ?? -1
            throw ProbeError(description: "Owned app failed: \(report["error"] as? String ?? "unknown child failure")")
        }
        try identity()
        launch["owned_pid"] = owned.processIdentifier; launch["status"] = "supervising"
        var samples: [[String: Any]] = [], posted: [[String: Any]] = []
        var lastSample = -1.0
        while !owned.isTerminated && ProcessInfo.processInfo.systemUptime - started < 60 {
            let elapsed = ProcessInfo.processInfo.systemUptime - started
            if elapsed - lastSample >= 0.25 {
                let front = NSWorkspace.shared.frontmostApplication
                samples.append(["elapsed": elapsed, "finished_launching": owned.isFinishedLaunching,
                    "active": owned.isActive, "activation_policy": owned.activationPolicy.rawValue,
                    "frontmost_pid": front?.processIdentifier ?? -1, "frontmost_bundle": front?.bundleIdentifier ?? ""])
                launch["samples"] = samples; lastSample = elapsed; try save()
            }
            if let request = try readJSON(out.appendingPathComponent("key-request.json")),
               let sequence = request["sequence"] as? Int, sequence > posted.count {
                try identity()
                let now = ProcessInfo.processInfo.systemUptime
                guard sequence == posted.count + 1 && sequence <= 64,
                      request["pid"] as? Int == Int(owned.processIdentifier),
                      request["supervisor_pid"] as? Int == Int(getpid()),
                      let requested = request["uptime"] as? Double, now >= requested && now - requested < 1,
                      request["key_window"] as? Bool == true,
                      request["field_is_first_responder"] as? Bool == true,
                      let windowNumber = request["window_number"] as? Int, windowNumber > 0,
                      let code = request["key"] as? UInt16, [4, 0, 15, 32, 59, 38, 36, 53].contains(code),
                      let down = request["down"] as? Bool, let flags = request["flags"] as? UInt64,
                      flags == 0 || flags == CGEventFlags.maskControl.rawValue else {
                    throw ProbeError(description: "Invalid, stale, or unowned virtual-key request")
                }
                guard let report = try childReport(), report["window_number"] as? Int == windowNumber,
                      let event = CGEvent(keyboardEventSource: eventSource, virtualKey: code, keyDown: down) else {
                    throw ProbeError(description: "Owned window evidence or virtual-key event is unavailable")
                }
                try require(CGPreflightPostEventAccess() && owned.isActive &&
                            NSWorkspace.shared.frontmostApplication?.processIdentifier == owned.processIdentifier,
                            "Owned stock field lost foreground or supervisor event-posting permission")
                event.flags = CGEventFlags(rawValue: flags); event.postToPid(owned.processIdentifier)
                let receipt: [String: Any] = ["sequence": sequence, "pid": owned.processIdentifier,
                    "supervisor_pid": getpid(), "key": code, "down": down, "flags": flags,
                    "request_uptime": requested, "posted_uptime": now, "posted": true]
                posted.append(receipt); launch["sent_keys"] = posted; try save()
                try writeJSON(receipt, to: out.appendingPathComponent("key-reply.json"))
            }
            RunLoop.main.run(until: Date().addingTimeInterval(0.025))
        }
        try require(owned.isTerminated, "Owned app did not exit within 60 seconds; inspect its cleanup evidence")
        guard let report = try childReport() else {
            throw ProbeError(description: "Launched app did not retain its report")
        }
        launch["child_status"] = report["status"]; launch["child_exit_code"] = report["child_exit_code"]
        try require(report["child_exit_code"] as? Int == 0 && report["finished_utc"] != nil &&
                    report["status"] as? String == "stock-control-ime-feasible",
                    "Owned app failed: \(report["error"] as? String ?? "no completed result")")
        try require(!posted.isEmpty && (report["sent_keys"] as? [[String: Any]])?.count == posted.count,
                    "Owned app did not acknowledge every supervisor-posted key")
        launch["status"] = "completed"; try save()
        print("macOS IME prerequisite: \(report["status"] ?? "unknown"); Bevy/candidate pixels remain unvalidated")
    } catch {
        launch["status"] = "failed"; launch["error"] = String(describing: error); try? save()
        if let owned = application, !owned.isTerminated {
            // Let the owned receiver unwind its normal input-source cleanup on
            // a driver error. Do not kill it or overwrite its still-live report.
            try? writeJSON(["pid": owned.processIdentifier, "supervisor_pid": getpid(),
                            "error": String(describing: error)], to: out.appendingPathComponent("driver-error.json"))
            let cleanupDeadline = ProcessInfo.processInfo.systemUptime + 5
            while !owned.isTerminated && ProcessInfo.processInfo.systemUptime < cleanupDeadline {
                RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            }
            launch["child_terminated_after_error"] = owned.isTerminated; try? save()
        }
        throw error
    }
}

final class ObservedTextView: NSTextView {
    var received: [[String: Any]] = []
    var inserted: [String] = []
    var markedCount = 0
    var escapeCount = 0
    var save: (() -> Void)?
    func state() -> [String: Any] {
        let value = string as NSString, marked = markedRange()
        let valid = marked.location != NSNotFound && marked.location <= value.length &&
            marked.length <= value.length - marked.location
        return ["storage": string, "has_marked_text": hasMarkedText(),
                "marked_range": rangeJSON(marked), "selection": rangeJSON(selectedRange()),
                "marked_text": valid ? value.substring(with: marked) : "",
                "committed": valid ? value.replacingCharacters(in: marked, with: "") : string,
                "input_source": inputContext?.selectedKeyboardInputSource ?? ""]
    }
    func record(_ method: String, _ fields: [String: Any] = [:]) {
        guard received.count < 1024 else { return }
        var entry = fields
        entry["method"] = method; entry["uptime"] = ProcessInfo.processInfo.systemUptime
        entry["state"] = state(); received.append(entry); save?()
    }
    override func keyDown(with event: NSEvent) {
        record("keyDown", ["key_code": event.keyCode, "flags": event.modifierFlags.rawValue])
        super.keyDown(with: event)
        if event.keyCode == 53 {
            escapeCount += 1
            record("escape:after", ["count": escapeCount])
        }
    }
    override func setMarkedText(_ value: Any, selectedRange: NSRange, replacementRange: NSRange) {
        markedCount += 1
        record("setMarkedText:before", ["text": stringValue(value), "selected": rangeJSON(selectedRange),
                                       "replacement": rangeJSON(replacementRange)])
        super.setMarkedText(value, selectedRange: selectedRange, replacementRange: replacementRange)
        record("setMarkedText:after")
    }
    override func insertText(_ value: Any, replacementRange: NSRange) {
        let text = stringValue(value)
        if !text.isEmpty { inserted.append(text) }
        record("insertText:before", ["text": text, "replacement": rangeJSON(replacementRange)])
        super.insertText(value, replacementRange: replacementRange)
        record("insertText:after")
    }
    override func unmarkText() {
        record("unmarkText:before"); super.unmarkText(); record("unmarkText:after")
    }
    override func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        let rect = super.firstRect(forCharacterRange: range, actualRange: actualRange)
        record("firstRect", ["requested": rangeJSON(range),
                             "screen_rect": ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]])
        return rect
    }
}

final class Probe {
    let out: URL
    var report: [String: Any] = ["schema_version": 1, "status": "started", "native_bevy_validated": false,
                                "candidate_placement": "not tested", "popup_pixels": "not captured"]
    var failure: String?
    init(_ out: URL) {
        self.out = out; report["started_utc"] = ISO8601DateFormatter().string(from: Date())
    }
    func save() {
        do {
            let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: out.appendingPathComponent("report.json"), options: .atomic)
        } catch { failure = "Cannot retain evidence: \(error)" }
    }
    func run(enable: Bool, exercise: Bool) throws {
        let environment = ProcessInfo.processInfo.environment
        report["requested"] = ["enable_japanese": enable, "exercise": exercise]
        report["environment"] = ["os": ProcessInfo.processInfo.operatingSystemVersionString,
            "image_os": environment["ImageOS"] ?? "", "image_version": environment["ImageVersion"] ?? "",
            "run_id": environment["GITHUB_RUN_ID"] ?? "", "sha": environment["GITHUB_SHA"] ?? "",
            "uid": getuid(), "pid": getpid(), "session": String(describing: CGSessionCopyCurrentDictionary())]
        report["application"] = ["bundle_id": Bundle.main.bundleIdentifier ?? "",
            "bundle_url": Bundle.main.bundleURL.path,
            "executable_url": Bundle.main.executableURL?.path ?? "", "parent_pid": getppid()]
        let before = sources(), prior = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        report["before"] = before.map(describe); report["prior_source"] = describe(prior)
        report["available_source_ids_before"] = sources(false).map(sourceID)
        report["event_posting_allowed"] = CGPreflightPostEventAccess()
        report["event_posting_role"] = exercise ? "receiver-only; supervised OS input" : "inventory-only"
        report["screen_capture_allowed"] = CGPreflightScreenCaptureAccess()
        report["accessibility_trusted"] = AXIsProcessTrusted()
        report["status"] = "inventory-complete"; save()
        if let error = failure { throw ProbeError(description: error) }
        guard enable || exercise else { return }
        let supervisorPID = Int32(environment["LIMO_CAD_IME_SUPERVISOR_PID"] ?? "") ?? -1
        func checkDriver() throws {
            if let error = try readJSON(out.appendingPathComponent("driver-error.json")),
               error["pid"] as? Int == Int(getpid()), error["supervisor_pid"] as? Int == Int(supervisorPID) {
                throw ProbeError(description: "Input supervisor stopped: \(error["error"] as? String ?? "unknown failure")")
            }
        }
        if exercise {
            let launch = try readJSON(out.appendingPathComponent("launch.json"))
            try require(supervisorPID > 0 && launch?["launcher_pid"] as? Int == Int(supervisorPID) &&
                        launch?["event_posting_allowed"] as? Bool == true,
                        "Expected the event-authorized input supervisor")
            report["input_supervisor_pid"] = supervisorPID; try checkDriver()
        }
        // Exact installed Apple Romaji-typing/Hiragana mode; fail with inventory if this SDK/OS differs.
        let matches = before.filter { sourceID($0) == "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese" }
        try require(matches.count == 1, "Expected one installed Apple Japanese Romaji-typing source; inspect inventory")
        var target = matches[0]
        let targetID = sourceID(target), parentID = "com.apple.inputmethod.Kotoeri.RomajiTyping"
        let parents = before.filter { sourceID($0) == parentID }
        try require(parents.count == 1 && property(target, kTISPropertyBundleID) as? String == parentID,
                    "Expected the installed Apple Romaji input method containing the Hiragana mode")
        report["parent_source_before"] = describe(parents[0])
        try require(property(target, kTISPropertyInputSourceIsSelectCapable) as? Bool == true, "Japanese mode is not selectable")
        let initiallyEnabledIDs = Set(before.filter { property($0, kTISPropertyInputSourceIsEnabled) as? Bool == true }.map(sourceID))
        var selectedByProbe = false
        defer {
            var cleanup: [String: Any] = [:]
            var cleanupErrors: [String] = []
            if selectedByProbe {
                let result = TISSelectInputSource(prior)
                let restored = sourceID(TISCopyCurrentKeyboardInputSource().takeRetainedValue())
                cleanup["restore_status"] = result; cleanup["restored_source"] = restored
                if result != noErr || restored != sourceID(prior) { cleanupErrors.append("Could not restore prior input source") }
            }
            // Japanese activation can lazily enable its Kana Palette after the
            // provisioning snapshot. Refresh the complete observed delta after
            // the exercise; never disable any source enabled before this probe.
            let enabledAtCleanup = sources().filter { property($0, kTISPropertyInputSourceIsEnabled) as? Bool == true }
            let newlyEnabled = enabledAtCleanup.filter { !initiallyEnabledIDs.contains(sourceID($0)) }
            cleanup["enabled_before_cleanup"] = enabledAtCleanup.map(sourceID).sorted()
            cleanup["newly_enabled"] = newlyEnabled.map(sourceID).sorted()
            var disabled: [[String: Any]] = []
            for source in newlyEnabled.sorted(by: { sourceID($0).count > sourceID($1).count }) {
                let result = TISDisableInputSource(source)
                disabled.append(["id": sourceID(source), "status": result])
                if result != noErr { cleanupErrors.append("Could not disable a source enabled by the probe: \(sourceID(source))") }
            }
            cleanup["disabled"] = disabled
            let finalEnabled = Set(sources().filter { property($0, kTISPropertyInputSourceIsEnabled) as? Bool == true }.map(sourceID))
            cleanup["enabled_set_restored"] = finalEnabled == initiallyEnabledIDs
            cleanup["unexpected_enabled"] = finalEnabled.subtracting(initiallyEnabledIDs).sorted()
            cleanup["unexpected_disabled"] = initiallyEnabledIDs.subtracting(finalEnabled).sorted()
            if finalEnabled != initiallyEnabledIDs { cleanupErrors.append("Enabled input-source set was not restored exactly") }
            cleanup["errors"] = cleanupErrors
            if !cleanupErrors.isEmpty { failure = cleanupErrors.joined(separator: "; ") }
            report["cleanup"] = cleanup; report["after"] = sources().map(describe); save()
        }
        // A mode can retain enabled=true while its containing input method is
        // disabled (observed on macos-15). Both must be enabled; inspecting only
        // the mode incorrectly skips provisioning and selection returns -50.
        var enableAttempts: [[String: Any]] = []
        for id in [parentID, targetID] {
            let current = sources().filter { sourceID($0) == id }
            try require(current.count == 1, "Installed Japanese source changed: \(id)")
            if property(current[0], kTISPropertyInputSourceIsEnabled) as? Bool == true { continue }
            try require(enable, "Japanese source is disabled; explicitly enable it on this disposable runner: \(id)")
            try require(property(current[0], kTISPropertyInputSourceIsEnableCapable) as? Bool == true,
                        "Installed Japanese source cannot be enabled: \(id)")
            let result = TISEnableInputSource(current[0])
            report["newly_enabled_after_provision"] = sources().filter {
                !initiallyEnabledIDs.contains(sourceID($0)) && property($0, kTISPropertyInputSourceIsEnabled) as? Bool == true
            }.map(sourceID).sorted()
            enableAttempts.append(["id": id, "status": result])
            report["enable_attempts"] = enableAttempts; save()
            try require(result == noErr, "TISEnableInputSource failed for \(id) with OSStatus \(result)")
            try require(sources().contains { sourceID($0) == id && property($0, kTISPropertyInputSourceIsEnabled) as? Bool == true },
                        "Source did not become enabled: \(id)")
        }
        report["enabled_japanese_sources"] = sources().filter { [parentID, targetID].contains(sourceID($0)) }.map(describe)
        report["available_source_ids"] = sources(false).map(sourceID); save()
        guard exercise else { report["status"] = "source-enable-feasible"; return }
        let app = NSApplication.shared
        let priorPolicy = app.activationPolicy()
        var policyChange: [String: Any] = ["before": priorPolicy.rawValue, "requested": priorPolicy != .regular]
        if priorPolicy != .regular { policyChange["accepted"] = app.setActivationPolicy(.regular) }
        policyChange["after"] = app.activationPolicy().rawValue
        report["activation_policy"] = policyChange; save()
        // A LaunchServices app is normally already regular. A no-op setter's
        // return value is not proof of its actual activation policy or focus.
        try require(app.activationPolicy() == .regular, "AppKit activation policy is not regular: \(policyChange)")
        let window = NSWindow(contentRect: NSRect(x: 160, y: 180, width: 640, height: 220),
                              styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.title = "Limo CAD disposable macOS IME probe"
        let field = ObservedTextView(frame: NSRect(x: 24, y: 40, width: 590, height: 140))
        field.font = NSFont.systemFont(ofSize: 24); field.isRichText = false
        window.contentView?.addSubview(field)
        window.makeKeyAndOrderFront(nil); window.makeFirstResponder(field)
        report["status"] = "exercise-in-progress"; report["window_number"] = window.windowNumber
        report["selected_source"] = describe(target); save()
        field.save = { [weak self, weak field] in
            guard let self = self, let field = field else { return }
            self.report["received"] = field.received; self.report["field"] = field.state(); self.save()
        }
        let started = ProcessInfo.processInfo.systemUptime
        var stage = 0, secondMarkedCount = 0, sent: [[String: Any]] = []
        report["stage"] = stage
        var activationRequested = false
        var activationSamples: [[String: Any]] = []
        func focus() throws {
            try require(NSWorkspace.shared.frontmostApplication?.processIdentifier == getpid() &&
                        window.isKeyWindow && window.firstResponder === field, "Owned stock field lost foreground/focus")
        }
        func key(_ code: CGKeyCode, _ down: Bool, _ flags: CGEventFlags = []) throws {
            try focus(); try checkDriver()
            let sequence = sent.count + 1
            let request: [String: Any] = ["sequence": sequence, "pid": getpid(), "supervisor_pid": supervisorPID,
                "key": code, "down": down, "flags": flags.rawValue,
                "uptime": ProcessInfo.processInfo.systemUptime, "window_number": window.windowNumber,
                "key_window": window.isKeyWindow, "field_is_first_responder": window.firstResponder === field]
            try writeJSON(request, to: out.appendingPathComponent("key-request.json"))
            let deadline = ProcessInfo.processInfo.systemUptime + 3
            while ProcessInfo.processInfo.systemUptime < deadline {
                try checkDriver()
                if let reply = try readJSON(out.appendingPathComponent("key-reply.json")),
                   reply["sequence"] as? Int == sequence {
                    try require(reply["pid"] as? Int == Int(getpid()) &&
                                reply["supervisor_pid"] as? Int == Int(supervisorPID) &&
                                reply["key"] as? UInt16 == code && reply["down"] as? Bool == down &&
                                reply["flags"] as? UInt64 == flags.rawValue && reply["posted"] as? Bool == true,
                                "Input supervisor returned an invalid virtual-key receipt")
                    sent.append(reply); try focus()
                    Thread.sleep(forTimeInterval: 0.025); return
                }
                Thread.sleep(forTimeInterval: 0.005)
            }
            throw ProbeError(description: "Input supervisor did not acknowledge virtual key \(sequence)")
        }
        func tap(_ code: CGKeyCode) throws { try key(code, true); try key(code, false) }
        func haru() throws { for code in [CGKeyCode(4), 0, 15, 32] { try tap(code) } }
        func hiragana() throws {
            try key(59, true, .maskControl)
            defer { try? key(59, false) }
            try key(38, true, .maskControl); try key(38, false, .maskControl)
        }
        func stop(_ timer: Timer) {
            timer.invalidate(); app.stop(nil)
            // Wake NSApplication's event wait after stop from a run-loop timer.
            // This lifecycle event is never sent to the text-input client.
            if let wake = NSEvent.otherEvent(with: .applicationDefined, location: .zero,
                modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber,
                context: nil, subtype: 0, data1: 0, data2: 0) {
                app.postEvent(wake, atStart: false)
            }
        }
        let timer = Timer(timeInterval: 0.25, repeats: true) { timer in
            do {
                try require(ProcessInfo.processInfo.systemUptime - started < 30, "IME deadline exceeded at stage \(stage)")
                try require(field.received.count < 1024, "Input callback trace exceeded its bound")
                try checkDriver()
                if let error = self.failure { throw ProbeError(description: error) }
                if stage == 0 {
                    // LaunchServices requested foreground activation. Ask once
                    // after launch, but never mistake that request for focus.
                    let front = NSWorkspace.shared.frontmostApplication
                    let activation: [String: Any] = ["finished_launching": NSRunningApplication.current.isFinishedLaunching,
                        "running": app.isRunning, "active": app.isActive, "key_window": window.isKeyWindow,
                        "window_visible": window.isVisible, "window_on_active_space": window.isOnActiveSpace,
                        "activation_policy": app.activationPolicy().rawValue,
                        "field_is_first_responder": window.firstResponder === field,
                        "frontmost_pid": front?.processIdentifier ?? -1, "frontmost_bundle": front?.bundleIdentifier ?? "",
                        "elapsed": ProcessInfo.processInfo.systemUptime - started,
                        "requested": activationRequested]
                    self.report["activation"] = activation
                    activationSamples.append(activation); self.report["activation_samples"] = activationSamples
                    if !activationRequested && NSRunningApplication.current.isFinishedLaunching {
                        if #available(macOS 14.0, *) { app.activate() }
                        else { app.activate(ignoringOtherApps: true) }
                        window.makeKeyAndOrderFront(nil); window.makeFirstResponder(field)
                        activationRequested = true
                    }
                    self.save()
                    if NSWorkspace.shared.frontmostApplication?.processIdentifier != getpid() || !window.isKeyWindow { return }
                    try focus()
                    // Re-resolve after app launch and provisioning; do not
                    // select a cached mode from the all-installed inventory.
                    let selectable = sources(false).filter { sourceID($0) == targetID }
                    self.report["available_source_ids_at_focus"] = sources(false).map(sourceID)
                    try require(selectable.count == 1 && property(selectable[0], kTISPropertyInputSourceIsSelectCapable) as? Bool == true,
                                "Enabled Japanese mode is not available for selection in the focused app")
                    target = selectable[0]; self.report["selected_source"] = describe(target)
                    selectedByProbe = true
                    let result = TISSelectInputSource(target)
                    self.report["select_status"] = result
                    try require(result == noErr, "TISSelectInputSource failed: \(result)")
                    stage = 1
                } else {
                    try focus()
                    let state = field.state()
                    if stage == 1 {
                        if field.inputContext?.selectedKeyboardInputSource != targetID { return }
                        try haru(); stage = 2
                    } else if stage == 2 && field.hasMarkedText() {
                        try hiragana(); stage = 3
                    } else if stage == 3 && state["marked_text"] as? String == "はる" {
                        try require(field.markedCount > 0 && state["committed"] as? String == "" && field.inserted.isEmpty,
                                    "Provisional keys changed committed text")
                        self.report["preedit"] = state; try tap(36); stage = 4
                    } else if stage == 4 && !field.hasMarkedText() && field.string == "はる" {
                        try require(field.inserted == ["はる"], "Return did not commit exactly once")
                        self.report["committed"] = state; secondMarkedCount = field.markedCount
                        try haru(); stage = 5
                    } else if stage == 5 && field.hasMarkedText() && field.markedCount > secondMarkedCount {
                        try hiragana(); stage = 6
                    } else if stage == 6 && state["marked_text"] as? String == "はる" {
                        try require(state["committed"] as? String == "はる" && field.inserted == ["はる"], "Second preedit changed committed text")
                        self.report["second_preedit"] = state; try tap(53)
                        self.report["escape_attempts"] = 1; stage = 7
                    } else if (stage == 7 || stage == 8) && !field.hasMarkedText() {
                        try require(field.string == "はる" && field.inserted == ["はる"], "Escape changed committed text")
                        self.report["cancelled"] = state; self.report["status"] = "stock-control-ime-feasible"
                        stop(timer)
                    } else if stage == 7 && field.escapeCount == 1 {
                        // Apple documents Escape both reverting a conversion to
                        // yomi and deleting text awaiting conversion. The first
                        // Escape can leave that yomi marked; permit exactly one
                        // further real Escape after the first was processed.
                        try require(state["marked_text"] as? String == "はる" &&
                                    state["committed"] as? String == "はる" && field.inserted == ["はる"],
                                    "First Escape changed the expected provisional or committed text")
                        self.report["first_escape"] = state; try tap(53)
                        self.report["escape_attempts"] = 2; stage = 8
                    }
                }
                self.report["stage"] = stage; self.report["sent_keys"] = sent; self.save()
            } catch {
                self.failure = String(describing: error); stop(timer)
            }
        }
        RunLoop.main.add(timer, forMode: .common); app.run(); timer.invalidate()
        report["received"] = field.received; report["sent_keys"] = sent; report["final_field"] = field.state()
        report["elapsed_seconds"] = ProcessInfo.processInfo.systemUptime - started
        field.save = nil; window.close()
        if let error = failure { throw ProbeError(description: error) }
        try require(report["status"] as? String == "stock-control-ime-feasible", "Owned probe closed before completion")
    }
}

var probe: Probe?
do {
    let env = ProcessInfo.processInfo.environment, args = Array(CommandLine.arguments.dropFirst())
    try require(env["GITHUB_ACTIONS"] == "true" && env["RUNNER_OS"] == "macOS" &&
                env["RUNNER_ENVIRONMENT"] == "github-hosted" && env["GITHUB_REPOSITORY_ID"] == "1313334315" &&
                env["GITHUB_RUN_ID"]?.range(of: "^[0-9]+$", options: .regularExpression) != nil, "Disposable GitHub macOS runner required")
    try require(args.count >= 2 && args[0] == "--out", "Expected --out directory")
    try require(args.dropFirst(2).allSatisfy { ["--enable-japanese", "--exercise", "--launch", "--app-child"].contains($0) }, "Unknown probe option")
    try require(!(args.contains("--launch") && args.contains("--app-child")), "Conflicting launch modes")
    guard let temp = env["RUNNER_TEMP"] else { throw ProbeError(description: "RUNNER_TEMP is absent") }
    let root = URL(fileURLWithPath: temp).resolvingSymlinksInPath().standardizedFileURL.path + "/"
    let out = URL(fileURLWithPath: args[1]).resolvingSymlinksInPath().standardizedFileURL
    try require(args[1].hasPrefix("/") && out.path.hasPrefix(root), "Output must be beneath RUNNER_TEMP")
    if args.contains("--launch") {
        try require(args.contains("--exercise"), "App launch is only needed for the input exercise")
        try launchProbe(out: out, arguments: args.filter { $0 != "--launch" }, environment: env)
        exit(0)
    }
    if args.contains("--app-child") {
        let bundleURL = out.appendingPathComponent("StockIMEProbe.app").resolvingSymlinksInPath()
        try require(Bundle.main.bundleURL.resolvingSymlinksInPath().path == bundleURL.path &&
                    Bundle.main.bundleIdentifier == "org.nobscad.qa.StockIMEProbe.run" + env["GITHUB_RUN_ID"]!,
                    "Expected the launched owned app bundle")
        let log = out.appendingPathComponent("probe.log").path
        try require(freopen(log, "a", stdout) != nil && freopen(log, "a", stderr) != nil,
                    "Cannot retain the launched app's log")
    }
    try require(!args.contains("--exercise") || args.contains("--app-child"),
                "Exercise must use the supervised LaunchServices app")
    let current = Probe(out); probe = current; current.save()
    try current.run(enable: args.contains("--enable-japanese"), exercise: args.contains("--exercise"))
    if let error = current.failure { throw ProbeError(description: error) }
    current.report["finished_utc"] = ISO8601DateFormatter().string(from: Date())
    current.report["child_exit_code"] = 0; current.save()
    if let error = current.failure { throw ProbeError(description: error) }
    print("macOS IME prerequisite: \(current.report["status"]!); Bevy/candidate pixels remain unvalidated")
} catch {
    probe?.report["status"] = "failed"; probe?.report["error"] = String(describing: error)
    probe?.report["child_exit_code"] = 1
    probe?.report["finished_utc"] = ISO8601DateFormatter().string(from: Date()); probe?.save()
    FileHandle.standardError.write(Data((String(describing: error) + "\n").utf8)); exit(1)
}
