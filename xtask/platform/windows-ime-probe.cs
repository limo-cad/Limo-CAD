// QA-only Windows IME prerequisite probe. Never sends Unicode or IME messages.
// COM layouts/order match the Windows SDK msctf.h; no product dependencies.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Forms;

public static class WindowsImeProbe {
    [StructLayout(LayoutKind.Sequential)] public struct Profile {
        public uint Type; public ushort Language; public Guid ClassId, ProfileId, Category;
        public IntPtr Substitute; public uint Capabilities; public IntPtr Layout; public uint Flags;
    }
    [ComImport, Guid("71c6e74d-0f28-11d8-a82a-00065b84435c"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface Profiles {
        [PreserveSig] int Clone(out Profiles copy);
        [PreserveSig] int Next(uint count, out Profile profile, out uint fetched);
        [PreserveSig] int Reset();
        [PreserveSig] int Skip(uint count);
    }
    [ComImport, Guid("71c6e74c-0f28-11d8-a82a-00065b84435c"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface ProfileManager {
        [PreserveSig] int Activate(uint type, ushort language, ref Guid clsid, ref Guid profile, IntPtr layout, uint flags);
        [PreserveSig] int Deactivate(uint type, ushort language, ref Guid clsid, ref Guid profile, IntPtr layout, uint flags);
        [PreserveSig] int Get(uint type, ushort language, ref Guid clsid, ref Guid profile, IntPtr layout, out Profile value);
        [PreserveSig] int Enumerate(ushort language, out Profiles profiles);
        [PreserveSig] int Release(ref Guid clsid, uint flags);
        [PreserveSig] int Register(ref Guid clsid, ushort language, ref Guid profile, IntPtr description, uint descriptionLength,
            IntPtr icon, uint iconLength, uint index, IntPtr substitute, uint preferred, int enabled, uint flags);
        [PreserveSig] int Unregister(ref Guid clsid, ushort language, ref Guid profile, uint flags);
        [PreserveSig] int Active(ref Guid category, out Profile profile);
    }
    // ITfInputProcessorProfiles, in Windows SDK vtable order. Only the current
    // user's existing Microsoft Japanese profile is enabled; no default-user API.
    [ComImport, Guid("1f02b6c5-7842-4ee6-8a0b-9a24183a95ca"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface UserProfiles {
        [PreserveSig] int Register(ref Guid clsid);
        [PreserveSig] int Unregister(ref Guid clsid);
        [PreserveSig] int Add(ref Guid clsid, ushort language, ref Guid profile, IntPtr description, uint descriptionLength, IntPtr icon, uint iconLength, uint iconIndex);
        [PreserveSig] int Remove(ref Guid clsid, ushort language, ref Guid profile);
        [PreserveSig] int EnumerateProcessors(out IntPtr enumerator);
        [PreserveSig] int GetDefault(ushort language, ref Guid category, out Guid clsid, out Guid profile);
        [PreserveSig] int SetDefault(ushort language, ref Guid clsid, ref Guid profile);
        [PreserveSig] int Activate(ref Guid clsid, ushort language, ref Guid profile);
        [PreserveSig] int GetActive(ref Guid clsid, out ushort language, out Guid profile);
        [PreserveSig] int Description(ref Guid clsid, ushort language, ref Guid profile, out IntPtr description);
        [PreserveSig] int CurrentLanguage(out ushort language);
        [PreserveSig] int ChangeLanguage(ushort language);
        [PreserveSig] int LanguageList(out IntPtr languages, out uint count);
        [PreserveSig] int EnumerateLanguage(ushort language, out IntPtr enumerator);
        [PreserveSig] int Enable(ref Guid clsid, ushort language, ref Guid profile, int enabled);
        [PreserveSig] int IsEnabled(ref Guid clsid, ushort language, ref Guid profile, out int enabled);
    }
    // The first five ITfThreadMgr methods, in SDK vtable order. Document-manager
    // pointers are observed and released only; this probe never supplies a store.
    [ComImport, Guid("aa80e801-2021-11d2-93e0-0060b067b86e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface ThreadManager {
        [PreserveSig] int Activate(out uint client);
        [PreserveSig] int Deactivate();
        [PreserveSig] int CreateDocument(out IntPtr document);
        [PreserveSig] int EnumerateDocuments(out IntPtr enumerator);
        [PreserveSig] int Focus(out IntPtr document);
    }
    static void RequireDisposableRunner() {
        if (Environment.GetEnvironmentVariable("GITHUB_ACTIONS") != "true"
            || Environment.GetEnvironmentVariable("RUNNER_OS") != "Windows"
            || Environment.GetEnvironmentVariable("RUNNER_ENVIRONMENT") != "github-hosted"
            || Environment.GetEnvironmentVariable("GITHUB_REPOSITORY_ID") != "1313334315"
            || !System.Text.RegularExpressions.Regex.IsMatch(Environment.GetEnvironmentVariable("GITHUB_RUN_ID") ?? "", @"^\d+$"))
            throw new InvalidOperationException("Profile changes and input require the disposable GitHub-hosted Limo-CAD Windows job");
    }
    public static object JapaneseProfileStatus() {
        var report = new Dictionary<string, object>();
        var clsid = new Guid("03b5835f-f03c-411b-9ce2-aa23e1171e36");
        var profile = new Guid("a76c93d9-5523-4e90-aafa-4db112f9ac76");
        var user = (UserProfiles)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("33c53a50-f456-4884-b049-85fd643ecfed")));
        try {
            int enabled; ushort language;
            int result = user.IsEnabled(ref clsid, 0x411, ref profile, out enabled);
            report["is_enabled_hresult"] = result.ToString("X8");
            report["is_enabled"] = result == 0 ? (object)(enabled != 0) : null;
            result = user.CurrentLanguage(out language);
            report["current_language_hresult"] = result.ToString("X8");
            report["current_language"] = result == 0 ? language.ToString("X4") : null;
        } finally { Marshal.ReleaseComObject(user); }
        var manager = Manager();
        try {
            Profile value;
            int result = manager.Get(1, 0x411, ref clsid, ref profile, IntPtr.Zero, out value);
            report["get_profile_hresult"] = result.ToString("X8");
            report["get_profile"] = result == 0 ? Describe(value) : null;
            var category = new Guid("34745c63-b2f0-4784-8b67-5e12c8701a31");
            result = manager.Active(ref category, out value);
            report["active_profile_hresult"] = result.ToString("X8");
            report["active_profile"] = result == 0 ? Describe(value) : null;
        } finally { Marshal.ReleaseComObject(manager); }
        report["thread_id"] = GetCurrentThreadId();
        return report;
    }
    public static object EnableJapaneseProfile() {
        RequireDisposableRunner();
        var report = new Dictionary<string, object> {
            {"api", "ITfInputProcessorProfiles::EnableLanguageProfile"}, {"scope", "current disposable user"},
            {"invoked", false}, {"succeeded", false}, {"before", JapaneseProfileStatus()}
        };
        var manager = (UserProfiles)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("33c53a50-f456-4884-b049-85fd643ecfed")));
        var clsid = new Guid("03b5835f-f03c-411b-9ce2-aa23e1171e36");
        var profile = new Guid("a76c93d9-5523-4e90-aafa-4db112f9ac76");
        try {
            // The legacy query and modern profile flags disagreed on the hosted
            // image. A true legacy query must not turn provisioning into a no-op
            // or be reported as an EnableLanguageProfile result we never called.
            report["invoked"] = true;
            int result = manager.Enable(ref clsid, 0x411, ref profile, 1);
            report["hresult"] = result.ToString("X8");
            report["succeeded"] = result == 0;
        } catch (Exception error) {
            report["error"] = error.ToString();
            report["hresult"] = error.HResult.ToString("X8");
        } finally { Marshal.ReleaseComObject(manager); }
        report["after"] = JapaneseProfileStatus();
        return report;
    }
    static ProfileManager Manager() {
        return (ProfileManager)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("33c53a50-f456-4884-b049-85fd643ecfed")));
    }
    static Dictionary<string, object> Describe(Profile p) {
        return new Dictionary<string, object> {
            {"type", p.Type}, {"language", p.Language.ToString("X4")}, {"class_id", p.ClassId.ToString()},
            {"profile_id", p.ProfileId.ToString()}, {"category", p.Category.ToString()},
            {"enabled", (p.Flags & 2) != 0}, {"active", (p.Flags & 1) != 0}, {"flags", p.Flags},
            {"layout", p.Layout.ToInt64().ToString("X")}, {"capabilities", p.Capabilities}
        };
    }
    public static object[] EnumerateProfiles() {
        var result = new List<object>(); var manager = Manager(); Profiles profiles = null;
        try {
            Marshal.ThrowExceptionForHR(manager.Enumerate(0, out profiles));
            for (int i = 0; i < 256; i++) {
                Profile profile; uint fetched;
                int hr = profiles.Next(1, out profile, out fetched);
                Marshal.ThrowExceptionForHR(hr);
                if (fetched == 0) return result.ToArray();
                result.Add(Describe(profile));
            }
            throw new InvalidOperationException("Unexpectedly more than 256 TSF profiles");
        } finally {
            if (profiles != null) Marshal.ReleaseComObject(profiles);
            Marshal.ReleaseComObject(manager);
        }
    }
    static Profile ActiveProfile() {
        var manager = Manager();
        try {
            var category = new Guid("34745c63-b2f0-4784-8b67-5e12c8701a31"); Profile result;
            Marshal.ThrowExceptionForHR(manager.Active(ref category, out result)); return result;
        } finally { Marshal.ReleaseComObject(manager); }
    }
    static bool Japanese(Profile profile) {
        return profile.Type == 1 && profile.Language == 0x411
            && profile.ClassId == new Guid("03b5835f-f03c-411b-9ce2-aa23e1171e36")
            && profile.ProfileId == new Guid("a76c93d9-5523-4e90-aafa-4db112f9ac76");
    }
    static bool SameProfile(Profile a, Profile b) {
        return a.Type == b.Type && a.Language == b.Language
            && (a.Type == 1 ? a.ClassId == b.ClassId && a.ProfileId == b.ProfileId : a.Layout == b.Layout);
    }
    // Fast, zero-key diagnosis. Registration/activation receipts are not proof of
    // IME delivery. All observations and cleanup run on the owned control's STA.
    public static object DiagnoseProfile() {
        RequireDisposableRunner();
        if (System.Threading.Thread.CurrentThread.GetApartmentState() != System.Threading.ApartmentState.STA || IntPtr.Size != 8)
            throw new InvalidOperationException("Profile diagnosis requires x64 STA Windows PowerShell");
        var observations = new List<object>();
        var cleanup = new Dictionary<string, object> { {"attempted", false}, {"status", "not-needed"} };
        var report = new Dictionary<string, object> {
            {"status", "failed"}, {"keys_sent", 0}, {"native_bevy_validated", false},
            {"ime_delivery_validated", false}, {"candidate_placement", "not tested"},
            {"observations", observations}, {"cleanup", cleanup}
        };
        ThreadManager thread = null; UserProfiles user = null; ProfileManager manager = null;
        bool threadActivated = false, sourceChangeAttempted = false;
        Profile previous = new Profile(); ushort previousLanguage = 0;
        var watch = Stopwatch.StartNew();
        using (var form = new Form()) using (var timer = new Timer()) {
            var field = new ObservedTextBox { Left = 24, Top = 40, Width = 540, ImeMode = ImeMode.On };
            Action<string> observe = label => {
                var sample = new Dictionary<string, object> {
                    {"stage", label}, {"elapsed_ms", watch.ElapsedMilliseconds},
                    {"profile", JapaneseProfileStatus()}, {"foreground", GetForegroundWindow().ToInt64()},
                    {"focus", GetFocus().ToInt64()}
                };
                if (thread != null) {
                    IntPtr document;
                    int hr = thread.Focus(out document);
                    sample["tsf_document_focus_hresult"] = hr.ToString("X8");
                    sample["tsf_document_present"] = document != IntPtr.Zero;
                    if (document != IntPtr.Zero) Marshal.Release(document);
                }
                observations.Add(sample);
            };
            Action requireFocus = () => {
                if (GetForegroundWindow() != form.Handle || GetFocus() != field.Handle)
                    throw new InvalidOperationException("Owned diagnosis control lost foreground/focus");
            };
            Action fail = () => { timer.Stop(); form.Close(); };
            int stage = 0;
            form.Text = "Limo CAD disposable TSF profile diagnosis (no keys)";
            form.Width = 620; form.Height = 180; form.StartPosition = FormStartPosition.CenterScreen; form.TopMost = true;
            form.Controls.Add(field);
            timer.Interval = 250;
            timer.Tick += (sender, args) => {
                try {
                    if (watch.ElapsedMilliseconds > 8000) throw new TimeoutException("Profile diagnosis timed out at stage " + stage);
                    requireFocus();
                    if (stage == 0) {
                        observe("owned-control-shown");
                        // CLSID_TF_ThreadMgr (Microsoft windows-sys 0.61.2,
                        // Win32/UI/TextServices); its fourth group is ab9e.
                        thread = (ThreadManager)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("529a9e6b-6587-4f23-ab9e-9c7d683e3c50")));
                        uint client; int hr = thread.Activate(out client);
                        report["thread_activate_hresult"] = hr.ToString("X8"); report["thread_client_id"] = client;
                        if (hr != 0) throw new InvalidOperationException("TSF thread activation did not return S_OK");
                        threadActivated = true; observe("thread-activated"); stage = 1;
                    } else if (stage == 1) {
                        // Observe after pumping messages before testing current-language semantics.
                        observe("thread-activated-after-pump");
                        manager = Manager();
                        user = (UserProfiles)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("33c53a50-f456-4884-b049-85fd643ecfed")));
                        previous = ActiveProfile();
                        int hr = user.CurrentLanguage(out previousLanguage);
                        report["prior_current_language_hresult"] = hr.ToString("X8");
                        report["prior_active_profile"] = Describe(previous);
                        report["prior_current_language"] = previousLanguage.ToString("X4");
                        if (hr != 0 || previous.Language != previousLanguage || (previous.Type != 1 && previous.Type != 2))
                            throw new InvalidOperationException("Cannot retain an exact restorable prior input source");
                        var clsid = new Guid("03b5835f-f03c-411b-9ce2-aa23e1171e36");
                        var profile = new Guid("a76c93d9-5523-4e90-aafa-4db112f9ac76");
                        Profile exact; hr = manager.Get(1, 0x411, ref clsid, ref profile, IntPtr.Zero, out exact);
                        report["registered_exact_profile_hresult"] = hr.ToString("X8");
                        if (hr != 0 || !Japanese(exact)) throw new InvalidOperationException("Exact Microsoft Japanese profile is not registered");
                        sourceChangeAttempted = true;
                        hr = user.ChangeLanguage(0x411);
                        report["change_language_hresult"] = hr.ToString("X8");
                        observe("after-change-current-language");
                        // Do not use DONTCARECURRENTINPUTLANGUAGE: it can report
                        // deferred activation, which is not active-source evidence.
                        requireFocus();
                        const uint EnableProfile = 0x00000001;
                        report["activate_flags"] = EnableProfile;
                        hr = manager.Activate(1, 0x411, ref clsid, ref profile, IntPtr.Zero, EnableProfile);
                        report["activate_profile_hresult"] = hr.ToString("X8");
                        observe("after-activate-exact-profile"); stage = 2;
                    } else {
                        observe("after-activation-pump");
                        report["owned_focus_at_final_observation"] = true;
                        report["status"] = "profile-diagnosis-complete"; timer.Stop(); form.Close();
                    }
                } catch (Exception ex) {
                    report["error"] = ex.ToString(); report["hresult"] = ex.HResult.ToString("X8"); fail();
                }
            };
            form.Shown += (sender, args) => {
                try {
                    report["pid"] = Process.GetCurrentProcess().Id; report["thread_id"] = GetCurrentThreadId();
                    report["window"] = form.Handle.ToInt64(); report["field_window"] = field.Handle.ToInt64();
                    SetForegroundWindow(form.Handle); field.Focus(); timer.Start();
                } catch (Exception ex) {
                    report["error"] = ex.ToString(); report["hresult"] = ex.HResult.ToString("X8"); fail();
                }
            };
            try {
                observe("before-owned-control");
                Application.Run(form);
            } catch (Exception ex) {
                report["error"] = ex.ToString(); report["hresult"] = ex.HResult.ToString("X8"); report["status"] = "failed";
            } finally {
                timer.Stop();
                if (sourceChangeAttempted) {
                    cleanup["attempted"] = true;
                    try {
                        int languageHr = user.ChangeLanguage(previousLanguage);
                        cleanup["change_language_hresult"] = languageHr.ToString("X8");
                        var priorClass = previous.Type == 1 ? previous.ClassId : Guid.Empty;
                        var priorProfile = previous.Type == 1 ? previous.ProfileId : Guid.Empty;
                        var priorLayout = previous.Type == 2 ? previous.Layout : IntPtr.Zero;
                        int activateHr = manager.Activate(previous.Type, previous.Language, ref priorClass,
                            ref priorProfile, priorLayout, 0);
                        cleanup["activate_profile_hresult"] = activateHr.ToString("X8");
                        ushort language; int currentHr = user.CurrentLanguage(out language);
                        Profile active = ActiveProfile();
                        cleanup["active_profile"] = Describe(active); cleanup["current_language"] = language.ToString("X4");
                        cleanup["current_language_hresult"] = currentHr.ToString("X8");
                        bool restored = languageHr == 0 && activateHr == 0 && currentHr == 0
                            && language == previousLanguage && SameProfile(active, previous);
                        cleanup["status"] = restored ? "restored" : "failed";
                        if (!restored) report["status"] = "failed";
                    } catch (Exception ex) {
                        cleanup["error"] = ex.ToString(); cleanup["hresult"] = ex.HResult.ToString("X8");
                        cleanup["status"] = "failed"; report["status"] = "failed";
                    }
                }
                if (threadActivated) {
                    try {
                        int hr = thread.Deactivate(); cleanup["thread_deactivate_hresult"] = hr.ToString("X8");
                        if (hr != 0) report["status"] = "failed";
                    } catch (Exception ex) {
                        cleanup["thread_deactivate_error"] = ex.ToString(); report["status"] = "failed";
                    }
                }
                if (manager != null) Marshal.ReleaseComObject(manager);
                if (user != null) Marshal.ReleaseComObject(user);
                if (thread != null) Marshal.ReleaseComObject(thread);
                report["elapsed_ms"] = watch.ElapsedMilliseconds;
                report["final_text"] = field.Text; report["received_ime_messages"] = field.ImeEvents;
            }
        }
        return report;
    }
    [DllImport("user32.dll")] static extern IntPtr GetProcessWindowStation();
    [DllImport("user32.dll")] static extern IntPtr GetThreadDesktop(uint thread);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool GetUserObjectInformation(IntPtr handle, int index, StringBuilder value, uint length, out uint needed);
    static string ObjectName(IntPtr handle) {
        var name = new StringBuilder(256); uint needed;
        if (!GetUserObjectInformation(handle, 2, name, 512, out needed))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return name.ToString();
    }
    public static object Desktop() {
        return new Dictionary<string, object> {
            {"station", ObjectName(GetProcessWindowStation())},
            {"desktop", ObjectName(GetThreadDesktop(GetCurrentThreadId()))},
            {"session_id", Process.GetCurrentProcess().SessionId}, {"user_interactive", Environment.UserInteractive}
        };
    }

    [StructLayout(LayoutKind.Sequential)] struct KeyboardInput {
        public ushort Key, Scan; public uint Flags, Time; public UIntPtr Extra;
    }
    [StructLayout(LayoutKind.Explicit, Size = 32)] struct InputUnion { [FieldOffset(0)] public KeyboardInput Keyboard; }
    [StructLayout(LayoutKind.Sequential)] struct Input { public uint Type; public InputUnion Data; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint count, Input[] input, int size);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern IntPtr GetFocus();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("imm32.dll")] static extern IntPtr ImmGetContext(IntPtr window);
    [DllImport("imm32.dll")] static extern bool ImmReleaseContext(IntPtr window, IntPtr context);
    [DllImport("imm32.dll", CharSet = CharSet.Unicode)] static extern int ImmGetCompositionStringW(IntPtr context, uint kind, byte[] value, uint length);
    static void Key(ushort key, bool up) {
        var input = new Input { Type = 1, Data = new InputUnion { Keyboard = new KeyboardInput { Key = key, Flags = up ? 2u : 0u } } };
        if (SendInput(1, new[] { input }, Marshal.SizeOf(typeof(Input))) != 1)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "SendInput failed");
    }
    static string Composition(IntPtr context, uint kind) {
        int length = ImmGetCompositionStringW(context, kind, null, 0);
        if (length < 0) throw new InvalidOperationException("ImmGetCompositionStringW returned " + length);
        if (length == 0) return "";
        if (length > 8192) throw new InvalidOperationException("Unexpected composition length");
        var bytes = new byte[length];
        int read = ImmGetCompositionStringW(context, kind, bytes, (uint)length);
        if (read < 0) throw new InvalidOperationException("ImmGetCompositionStringW read returned " + read);
        return Encoding.Unicode.GetString(bytes, 0, read);
    }
    sealed class ObservedTextBox : TextBox {
        public readonly List<object> ImeEvents = new List<object>();
        public string LastPreedit = "", Result = "", Error;
        public int Starts, Ends, Results;
        protected override void WndProc(ref Message message) {
            try {
                if (message.Msg == 0x10d) { Starts++; ImeEvents.Add(new { message = "WM_IME_STARTCOMPOSITION" }); }
                if (message.Msg == 0x10e) { Ends++; ImeEvents.Add(new { message = "WM_IME_ENDCOMPOSITION" }); }
                if (message.Msg == 0x10f) {
                    uint flags = unchecked((uint)message.LParam.ToInt64());
                    IntPtr context = ImmGetContext(Handle);
                    try {
                        if ((flags & 8) != 0) {
                            LastPreedit = Composition(context, 8);
                            ImeEvents.Add(new { message = "WM_IME_COMPOSITION", flags = flags, preedit = LastPreedit });
                        }
                        if ((flags & 0x800) != 0) {
                            Result = Composition(context, 0x800); Results++;
                            ImeEvents.Add(new { message = "WM_IME_COMPOSITION", flags = flags, result = Result });
                        }
                    } finally { ImmReleaseContext(Handle, context); }
                }
            } catch (Exception ex) { Error = ex.ToString(); }
            base.WndProc(ref message);
        }
    }
    // Called only by the guarded CI script. This is an owned stock textbox,
    // not the Bevy host; success means environment feasibility only.
    public static object Exercise() {
        RequireDisposableRunner();
        if (System.Threading.Thread.CurrentThread.GetApartmentState() != System.Threading.ApartmentState.STA)
            throw new InvalidOperationException("Run the probe in STA Windows PowerShell");
        if (IntPtr.Size != 8) throw new InvalidOperationException("The probe targets the x64 hosted runner");
        var report = new Dictionary<string, object> {
            {"status", "failed"}, {"event_source", "ordinary virtual-key SendInput through Windows Microsoft Japanese IME"},
            {"native_bevy_validated", false}, {"candidate_placement", "not tested"}, {"popup_pixels", "not captured"}
        };
        using (var form = new Form()) using (var timer = new Timer()) {
            form.Text = "Limo CAD disposable IME prerequisite probe";
            form.Width = 620; form.Height = 180; form.StartPosition = FormStartPosition.CenterScreen; form.TopMost = true;
            var field = new ObservedTextBox { Left = 24, Top = 40, Width = 540, ImeMode = ImeMode.On };
            form.Controls.Add(field);
            var keys = new List<object>(); var profiles = new List<object>();
            var watch = Stopwatch.StartNew(); int stage = 0, switches = 0, escapes = 0, escapeAfterEvents = 0;
            Action requireFocus = () => {
                if (GetForegroundWindow() != form.Handle || GetFocus() != field.Handle)
                    throw new InvalidOperationException("Owned probe field lost foreground/focus; no further keys sent");
            };
            Action<ushort, ushort> chord = (modifier, key) => {
                requireFocus(); keys.Add(new { modifier = modifier, key = key, elapsed_ms = watch.ElapsedMilliseconds });
                if (modifier != 0) Key(modifier, false);
                try { Key(key, false); Key(key, true); }
                finally { if (modifier != 0) Key(modifier, true); }
            };
            Action typeHaru = () => { foreach (ushort key in new ushort[] { 0x48, 0x41, 0x52, 0x55 }) chord(0, key); };
            timer.Interval = 400;
            timer.Tick += (sender, args) => {
                try {
                    if (watch.ElapsedMilliseconds > 20000) throw new TimeoutException("IME feasibility probe timed out at stage " + stage);
                    if (field.Error != null) throw new InvalidOperationException(field.Error);
                    requireFocus();
                    if (stage == 0) {
                        Profile active = ActiveProfile(); profiles.Add(Describe(active));
                        if (!Japanese(active)) {
                            if (++switches > 8) throw new InvalidOperationException("Microsoft Japanese IME did not activate after eight Win+Space switches");
                            chord(0x5b, 0x20); return;
                        }
                        report["active_profile"] = Describe(active); chord(0x11, 0x14); stage = 1;
                    } else if (stage == 1) { typeHaru(); stage = 2; }
                    else if (stage == 2 && field.LastPreedit == "\u306f\u308b" && field.Starts > 0) {
                        report["preedit"] = field.LastPreedit; report["results_before_commit"] = field.Results;
                        if (field.Results != 0) throw new InvalidOperationException("IME committed before Enter");
                        chord(0, 0x0d); stage = 3;
                    } else if (stage == 3 && field.Result == "\u306f\u308b" && field.Text == "\u306f\u308b") {
                        report["committed"] = field.Text; field.LastPreedit = ""; typeHaru(); stage = 4;
                    } else if (stage == 4 && field.LastPreedit == "\u306f\u308b" && field.Starts >= 2) {
                        escapeAfterEvents = field.ImeEvents.Count;
                        chord(0, 0x1b); escapes = 1; stage = 5;
                    } else if (stage == 5 && field.Ends >= 2 && field.Text == "\u306f\u308b") {
                        if (field.Results != 1) throw new InvalidOperationException("Composition did not commit exactly once");
                        report["cancelled_text"] = field.Text; report["status"] = "stock-control-ime-feasible";
                        timer.Stop(); form.Close();
                    } else if (stage == 5 && escapes == 1 && field.Starts > field.Ends
                        && field.ImeEvents.Count > escapeAfterEvents && field.LastPreedit == "\u306f\u308b") {
                        // Run 36362996743 received unchanged marked Hiragana
                        // after Escape: the first key dismissed conversion UI.
                        // Only a still-active, newly observed composition may
                        // receive one second Escape; never dismiss the form.
                        if (field.Results != 1 || field.Text != "\u306f\u308b")
                            throw new InvalidOperationException("First Escape changed accepted text or committed again");
                        chord(0, 0x1b); escapes = 2;
                    }
                } catch (Exception ex) {
                    report["error"] = ex.ToString(); report["hresult"] = ex.HResult.ToString("X8");
                    timer.Stop(); form.Close();
                }
            };
            form.Shown += (sender, args) => {
                report["pid"] = Process.GetCurrentProcess().Id; report["window"] = form.Handle.ToInt64();
                report["field_window"] = field.Handle.ToInt64();
                SetForegroundWindow(form.Handle); field.Focus(); timer.Start();
            };
            form.FormClosing += (sender, args) => { report["final_text"] = field.Text; };
            Application.Run(form);
            report["elapsed_ms"] = watch.ElapsedMilliseconds; report["keys"] = keys;
            report["profiles_observed_on_ui_thread"] = profiles; report["received_ime_messages"] = field.ImeEvents;
            report["escape_count"] = escapes; report["result_count"] = field.Results;
            report["composition_starts"] = field.Starts; report["composition_ends"] = field.Ends;
        }
        return report;
    }
}
