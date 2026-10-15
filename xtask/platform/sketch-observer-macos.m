// Read-only companion for investigating an unchanged CAD application.
// No event taps, input insertion, MCP requests, or CAD data writes.
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CommonCrypto/CommonDigest.h>
#include <fcntl.h>
#include <libproc.h>
#include <sys/stat.h>
#include <unistd.h>

static NSString *const TargetRelativePath = @"Code/noBS-CAD/artifacts/sketch-commit-scale-71d78963/Limo CAD Sketch Fix Test.app";

static double uptime(void) { return NSProcessInfo.processInfo.systemUptime; }
static NSDictionary *pointJSON(CGPoint p) { return @{ @"x": @(p.x), @"y": @(p.y) }; }
static NSDictionary *rectJSON(CGRect r) {
    return @{ @"x": @(r.origin.x), @"y": @(r.origin.y), @"width": @(r.size.width), @"height": @(r.size.height) };
}
static id attribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = NULL;
    return AXUIElementCopyAttributeValue(element, name, &value) == kAXErrorSuccess ? CFBridgingRelease(value) : nil;
}
static BOOL elementRect(AXUIElementRef element, CGRect *rect) {
    id position = attribute(element, kAXPositionAttribute), size = attribute(element, kAXSizeAttribute);
    return position && size && CFGetTypeID((__bridge CFTypeRef)position) == AXValueGetTypeID()
        && CFGetTypeID((__bridge CFTypeRef)size) == AXValueGetTypeID()
        && AXValueGetValue((__bridge AXValueRef)position, kAXValueCGPointType, &rect->origin)
        && AXValueGetValue((__bridge AXValueRef)size, kAXValueCGSizeType, &rect->size);
}
static BOOL drawingLabel(id value) {
    return [value isKindOfClass:NSString.class] && [value hasPrefix:@"Drawing "] && [value length] <= 64;
}
static NSString *processPath(pid_t pid) {
    char buffer[PROC_PIDPATHINFO_MAXSIZE] = {0};
    return proc_pidpath(pid, buffer, sizeof(buffer)) > 0 ? @(buffer) : nil;
}
static CGPoint relativePoint(CGPoint cursor, CGRect client, CGPoint viewportOrigin) {
    return CGPointMake(cursor.x - client.origin.x - viewportOrigin.x, cursor.y - client.origin.y - viewportOrigin.y);
}

// AccessKit puts the application content below the native window. Preserve
// candidates and only derive viewport positions when a content group is found.
static void discover(AXUIElementRef element, NSUInteger depth, NSUInteger *remaining,
                     NSMutableArray *fields, NSMutableArray *content, CGRect outer) {
    if (!*remaining || depth > 24) return;
    --*remaining;
    NSString *role = attribute(element, kAXRoleAttribute);
    NSString *label = attribute(element, kAXTitleAttribute);
    if (!drawingLabel(label)) label = attribute(element, kAXDescriptionAttribute);
    if (drawingLabel(label)) [fields addObject:(__bridge id)element];
    CGRect rect;
    if (depth <= 3 && [role isEqualToString:(__bridge NSString *)kAXGroupRole]
        && elementRect(element, &rect) && rect.size.width > outer.size.width * .8
        && rect.size.height > outer.size.height * .7 && CGRectContainsRect(CGRectInset(outer, -1, -1), rect)) {
        [content addObject:rectJSON(rect)];
    }
    NSArray *children = attribute(element, kAXChildrenAttribute);
    if ([children isKindOfClass:NSArray.class]) {
        for (id child in children) discover((__bridge AXUIElementRef)child, depth + 1, remaining, fields, content, outer);
    }
}

@interface SketchObserver : NSObject <NSApplicationDelegate, NSWindowDelegate>
@property NSWindow *window;
@property NSTextField *status;
@property NSButton *toggle;
@property NSURL *target;
@property NSURL *output;
@property NSURL *lastLog;
@property dispatch_queue_t queue;
@property dispatch_source_t timer;
@property NSRunningApplication *application;
@property NSFileHandle *file;
@property NSMutableArray *fields;
@property NSArray *contentCandidates;
@property CGRect outer;
@property double discoveredAt;
@property double beganAt;
@property double lastStatus;
@property NSUInteger bytes;
@property NSUInteger samples;
@property NSUInteger withDimensions;
@property BOOL lastForeground;
@property BOOL recording;
@end

@implementation SketchObserver
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    (void)notification;
    self.target = [NSURL fileURLWithPath:[NSHomeDirectory() stringByAppendingPathComponent:TargetRelativePath]];
    self.output = [NSURL fileURLWithPath:[NSHomeDirectory() stringByAppendingPathComponent:@"Desktop/Limo CAD Original Cursor Recording"]];
    self.queue = dispatch_queue_create("org.limocad.original-cursor-observer", DISPATCH_QUEUE_SERIAL);
    self.window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 570, 300)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable
        backing:NSBackingStoreBuffered defer:NO];
    self.window.title = @"Limo CAD — Original Cursor Recorder";
    self.window.releasedWhenClosed = NO;
    self.window.delegate = self;
    NSTextField *explanation = [NSTextField wrappingLabelWithString:
        @"Record the original Sketch Fix Test app while you draw a rectangle. This companion reads the cursor and drawing fields from that app without changing it.\n\n1. Open the original app.\n2. Start recording here, then return to CAD and reproduce the problem.\n3. Come back here and stop recording."];
    explanation.frame = NSMakeRect(24, 125, 522, 152);
    [self.window.contentView addSubview:explanation];
    NSButton *open = [NSButton buttonWithTitle:@"Open original app" target:self action:@selector(openOriginal:)];
    open.frame = NSMakeRect(20, 80, 170, 32);
    [self.window.contentView addSubview:open];
    self.toggle = [NSButton buttonWithTitle:@"Start recording" target:self action:@selector(toggleRecording:)];
    self.toggle.frame = NSMakeRect(198, 80, 170, 32);
    [self.window.contentView addSubview:self.toggle];
    NSButton *folder = [NSButton buttonWithTitle:@"Show recordings" target:self action:@selector(showRecordings:)];
    folder.frame = NSMakeRect(376, 80, 170, 32);
    [self.window.contentView addSubview:folder];
    self.status = [NSTextField wrappingLabelWithString:@"Ready. macOS Accessibility access is needed to read the live dimension fields."];
    self.status.frame = NSMakeRect(24, 18, 522, 54);
    self.status.font = [NSFont systemFontOfSize:12];
    [self.window.contentView addSubview:self.status];
    NSMenu *menu = [NSMenu new];
    NSMenuItem *item = [NSMenuItem new];
    NSMenu *applicationMenu = [NSMenu new];
    [applicationMenu addItemWithTitle:@"Quit recorder" action:@selector(terminate:) keyEquivalent:@"q"];
    item.submenu = applicationMenu;
    [menu addItem:item];
    NSApp.mainMenu = menu;
    [self.window center];
    [self.window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
}
- (void)openOriginal:(id)sender {
    (void)sender;
    NSWorkspaceOpenConfiguration *configuration = [NSWorkspaceOpenConfiguration configuration];
    configuration.allowsRunningApplicationSubstitution = NO;
    configuration.addsToRecentItems = NO;
    [NSWorkspace.sharedWorkspace openApplicationAtURL:self.target configuration:configuration
        completionHandler:^(NSRunningApplication *application, NSError *error) {
        (void)application;
        if (error) dispatch_async(dispatch_get_main_queue(), ^{ self.status.stringValue = error.localizedDescription; });
    }];
}
- (void)showRecordings:(id)sender {
    (void)sender;
    [NSWorkspace.sharedWorkspace activateFileViewerSelectingURLs:@[self.lastLog ?: self.output]];
}
- (void)toggleRecording:(id)sender {
    (void)sender;
    self.toggle.enabled = NO;
    if (self.recording) {
        dispatch_async(self.queue, ^{ [self finish:@"Stopped; recording saved."]; });
        return;
    }
    if (!AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)@{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES})) {
        self.status.stringValue = @"Allow Original Cursor Recorder in System Settings → Privacy & Security → Accessibility, then click Start again.";
        self.toggle.enabled = YES;
        return;
    }
    NSString *expected = [self.target.path stringByAppendingPathComponent:@"Contents/MacOS/limo-cad"];
    NSMutableArray *matches = [NSMutableArray new];
    for (NSRunningApplication *application in NSWorkspace.sharedWorkspace.runningApplications) {
        if ([processPath(application.processIdentifier) isEqualToString:expected]) [matches addObject:application];
    }
    if (matches.count != 1) {
        self.status.stringValue = @"Open exactly one original Sketch Fix Test app using the button above, then start recording.";
        self.toggle.enabled = YES;
        return;
    }
    self.application = matches.firstObject;
    dispatch_async(self.queue, ^{ [self begin:expected]; });
}
- (BOOL)write:(NSDictionary *)row {
    NSError *error;
    NSData *data = [NSJSONSerialization dataWithJSONObject:row options:NSJSONWritingSortedKeys error:&error];
    if (!data || self.bytes + data.length + 1 > 128 * 1024 * 1024) return NO;
    NSMutableData *line = [data mutableCopy];
    [line appendBytes:"\n" length:1];
    @try { [self.file writeData:line]; } @catch (NSException *exception) { return NO; }
    self.bytes += line.length;
    return YES;
}
- (void)begin:(NSString *)expected {
    NSError *error;
    if (![NSFileManager.defaultManager createDirectoryAtURL:self.output withIntermediateDirectories:YES
        attributes:@{NSFilePosixPermissions: @0700} error:&error]) {
        [self finish:error.localizedDescription]; return;
    }
    self.lastLog = [self.output URLByAppendingPathComponent:[NSString stringWithFormat:@"original-cursor-%@.jsonl", NSUUID.UUID.UUIDString]];
    int fd = open(self.lastLog.fileSystemRepresentation, O_CREAT | O_EXCL | O_WRONLY | O_CLOEXEC, 0600);
    if (fd < 0) { [self finish:@"Could not create the recording."]; return; }
    self.file = [[NSFileHandle alloc] initWithFileDescriptor:fd closeOnDealloc:YES];
    self.bytes = self.samples = self.withDimensions = 0;
    self.lastForeground = NO;
    self.beganAt = uptime(); self.discoveredAt = self.lastStatus = 0;
    self.fields = [NSMutableArray new]; self.contentCandidates = @[];
    NSData *engine = [NSData dataWithContentsOfFile:expected options:NSDataReadingMappedIfSafe error:&error];
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    NSMutableString *hash = [NSMutableString new];
    if (engine && engine.length <= UINT32_MAX) {
        CC_SHA256(engine.bytes, (CC_LONG)engine.length, digest);
        for (NSUInteger i = 0; i < sizeof(digest); ++i) [hash appendFormat:@"%02x", digest[i]];
    }
    if (![hash isEqualToString:@"71ed50af00ec242578809c4780340ea5fed4beacdaad56d6fa7edb95cc78f4be"]) {
        [self finish:@"The original CAD executable has changed; recording was not started."]; return;
    }
    self.recording = YES;
    if (![self write:@{ @"event": @"header", @"schema": @1,
        @"created_utc": [[NSISO8601DateFormatter new] stringFromDate:NSDate.date] }]) {
        [self finish:@"Could not write the recording header."]; return;
    }
    if (![self write:@{ @"event": @"identity", @"target_pid": @(self.application.processIdentifier), @"target_executable": expected,
        @"target_sha256": hash, @"raw_coordinates": @"Unrounded Quartz global screen points; top-left origin. They are OS cursor positions, not device motion or CAD-processed input.",
        @"dimension_values": @"Accessibility values from the original app's Drawing fields; not pixel OCR.",
        @"pairing": @"Independent polling: cursor before/after and per-field read intervals bound each sample. Samples are not a single CAD frame.",
        @"viewport_derivation": @"Optional, build-specific: AX content group plus UI scale inferred from a 142-unit Drawing field; raw coordinates and bounds are always retained.",
        @"cad_changes": @"none; no input insertion or MCP requests", @"interval_seconds": @(1. / 60.) }]) {
        [self finish:@"Could not write the recording identity."]; return;
    }
    CGDirectDisplayID displays[16];
    uint32_t count = 0;
    if (CGGetActiveDisplayList(16, displays, &count) == kCGErrorSuccess) {
        NSMutableArray *metadata = [NSMutableArray new];
        for (uint32_t index = 0; index < count; ++index) {
            [metadata addObject:@{ @"display_id": @(displays[index]), @"quartz_bounds": rectJSON(CGDisplayBounds(displays[index])),
                @"pixel_width": @(CGDisplayPixelsWide(displays[index])), @"pixel_height": @(CGDisplayPixelsHigh(displays[index])) }];
        }
        if (![self write:@{ @"event": @"displays", @"displays": metadata }]) {
            [self finish:@"Could not write display metadata."]; return;
        }
    }
    self.timer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, self.queue);
    dispatch_source_set_timer(self.timer, DISPATCH_TIME_NOW, NSEC_PER_SEC / 60, NSEC_PER_MSEC);
    dispatch_source_set_event_handler(self.timer, ^{ @autoreleasepool { [self sample:expected]; } });
    dispatch_resume(self.timer);
    dispatch_async(dispatch_get_main_queue(), ^{
        self.toggle.title = @"Stop recording"; self.toggle.enabled = YES;
        self.status.stringValue = @"Recording. Return to the original CAD app and draw the first rectangle.";
    });
}
- (void)refresh:(pid_t)pid {
    self.discoveredAt = uptime();
    [self.fields removeAllObjects]; self.contentCandidates = @[]; self.outer = CGRectNull;
    AXUIElementRef application = AXUIElementCreateApplication(pid);
    AXUIElementSetMessagingTimeout(application, .05);
    id window = attribute(application, kAXFocusedWindowAttribute);
    if (!window) {
        NSArray *windows = attribute(application, kAXWindowsAttribute);
        if ([windows isKindOfClass:NSArray.class] && windows.count == 1) window = windows.firstObject;
    }
    NSMutableArray *content = [NSMutableArray new];
    if (window && elementRect((__bridge AXUIElementRef)window, &_outer)) {
        NSUInteger remaining = 2000;
        discover((__bridge AXUIElementRef)window, 0, &remaining, self.fields, content, self.outer);
    }
    self.contentCandidates = content;
    CFRelease(application);
}
- (void)sample:(NSString *)expected {
    pid_t pid = self.application.processIdentifier;
    if (self.application.terminated || ![processPath(pid) isEqualToString:expected]) {
        [self finish:@"Original app exited or changed; recording saved."]; return;
    }
    double start = uptime();
    if (start - self.beganAt >= 15 * 60) { [self finish:@"15-minute limit reached; recording saved."]; return; }
    BOOL foreground = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier == pid;
    if (foreground != self.lastForeground) {
        [self write:@{ @"event": @"target_foreground", @"uptime": @(start), @"foreground": @(foreground) }];
        self.lastForeground = foreground;
    }
    if (!foreground) return;
    CGEventRef event = CGEventCreate(NULL);
    if (!event) { [self finish:@"Could not read the OS cursor; recording saved."]; return; }
    CGPoint cursor = CGEventGetLocation(event); CFRelease(event);
    if (start - self.discoveredAt > 1.) [self refresh:pid];
    NSMutableArray *dimensions = [NSMutableArray new];
    for (id field in self.fields) {
        double before = uptime();
        AXUIElementRef element = (__bridge AXUIElementRef)field;
        NSString *label = attribute(element, kAXTitleAttribute);
        if (!drawingLabel(label)) label = attribute(element, kAXDescriptionAttribute);
        id value = attribute(element, kAXValueAttribute);
        CGRect bounds;
        if (!drawingLabel(label) || ![value isKindOfClass:NSString.class] || [value length] > 256 || !elementRect(element, &bounds)) continue;
        [dimensions addObject:@{ @"label": label, @"value": value, @"screen_bounds": rectJSON(bounds),
            @"read_start_uptime": @(before), @"read_end_uptime": @(uptime()) }];
    }
    event = CGEventCreate(NULL);
    if (!event) { [self finish:@"Could not read the OS cursor; recording saved."]; return; }
    CGPoint after = CGEventGetLocation(event); CFRelease(event);
    if (NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier != pid) return;
    NSMutableDictionary *row = [@{ @"event": @"sample", @"sequence": @(++self.samples), @"start_uptime": @(start),
        @"end_uptime": @(uptime()), @"cursor_screen_before": pointJSON(cursor), @"cursor_screen_after": pointJSON(after),
        @"dimensions": dimensions, @"content_candidates": self.contentCandidates } mutableCopy];
    if (!CGRectIsNull(self.outer)) row[@"window_bounds"] = rectJSON(self.outer);
    // This optional derived field is explicitly weaker than the raw capture.
    // Default solid/sketch chrome in source 71d78963 uses 240/120/48 UI units.
    if (self.contentCandidates.count == 1 && dimensions.count) {
        NSDictionary *c = self.contentCandidates.firstObject;
        CGRect client = CGRectMake([c[@"x"] doubleValue], [c[@"y"] doubleValue], [c[@"width"] doubleValue], [c[@"height"] doubleValue]);
        double scale = [dimensions.firstObject[@"screen_bounds"][@"width"] doubleValue] / 142.;
        if (scale > .4 && scale < 4.) {
            CGPoint origin = CGPointMake(fmin(240 * scale, client.size.width * .45), fmin(120 * scale, client.size.height * .3));
            row[@"viewport_derived_build_71d78963"] = @{ @"cursor_points": pointJSON(relativePoint(cursor, client, origin)),
                @"origin_in_client_points": pointJSON(origin), @"inferred_ui_scale": @(scale),
                @"width_points": @(client.size.width - origin.x),
                @"height_points": @(client.size.height - origin.y - fmin(48 * scale, client.size.height * .1)) };
        }
    }
    if (dimensions.count) ++self.withDimensions;
    if (![self write:row]) { [self finish:@"Recording stopped at its size limit or a disk error."]; return; }
    if (start - self.lastStatus > 1.) {
        self.lastStatus = start;
        NSUInteger count = dimensions.count;
        dispatch_async(dispatch_get_main_queue(), ^{ self.status.stringValue = count
            ? [NSString stringWithFormat:@"Recording cursor and %lu drawing fields. Reproduce the problem, then stop here.", (unsigned long)count]
            : @"Recording cursor; waiting for the rectangle's live dimension fields."; });
    }
}
- (void)finish:(NSString *)message {
    if (self.timer) { dispatch_source_cancel(self.timer); self.timer = nil; }
    if (self.file) {
        [self write:@{ @"event": @"end", @"uptime": @(uptime()), @"samples": @(self.samples),
            @"samples_with_dimensions": @(self.withDimensions), @"reason": message ?: @"Stopped" }];
        @try { [self.file synchronizeFile]; [self.file closeFile]; } @catch (NSException *exception) { (void)exception; }
        self.file = nil;
    }
    self.recording = NO;
    dispatch_async(dispatch_get_main_queue(), ^{
        self.toggle.title = @"Start recording"; self.toggle.enabled = YES;
        self.status.stringValue = [NSString stringWithFormat:@"%@\n%lu cursor samples; %lu with drawing values.", message,
            (unsigned long)self.samples, (unsigned long)self.withDimensions];
    });
}
- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)sender { (void)sender; return YES; }
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    (void)sender;
    dispatch_sync(self.queue, ^{ [self finish:@"Recorder closed; recording saved."]; });
    return NSTerminateNow;
}
@end

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc == 2 && strcmp(argv[1], "--self-test") == 0) {
            CGPoint point = relativePoint(CGPointMake(-1234.567890123, 333.765432109), CGRectMake(-1440, 22, 1360, 860), CGPointMake(240, 120));
            NSCAssert(fabs(point.x - (-34.567890123)) < 1e-10 && fabs(point.y - 191.765432109) < 1e-10, @"Subpixel and secondary-display coordinates must survive conversion");
            NSCAssert(drawingLabel(@"Drawing width") && !drawingLabel(@"Width") && !drawingLabel(@42), @"Only the owned Drawing fields are collected");
            NSData *json = [NSJSONSerialization dataWithJSONObject:pointJSON(point) options:0 error:NULL];
            NSDictionary *roundtrip = [NSJSONSerialization JSONObjectWithData:json options:0 error:NULL];
            NSCAssert([roundtrip[@"x"] doubleValue] == point.x && [roundtrip[@"y"] doubleValue] == point.y, @"JSON must retain the raw floating-point values");
            puts("PASS: coordinate precision, field filtering, JSON precision. Live Accessibility recording is not qualified.");
            return 0;
        }
        if (argc == 2 && strcmp(argv[1], "--permission-status") == 0) {
            printf("accessibility_trusted=%s\n", AXIsProcessTrusted() ? "true" : "false");
            return 0;
        }
        NSApplication *application = NSApplication.sharedApplication;
        [application setActivationPolicy:NSApplicationActivationPolicyRegular];
        SketchObserver *delegate = [SketchObserver new];
        application.delegate = delegate;
        [application run];
    }
    return 0;
}
