// Platform bridge for Adobe's DoScript Apple event. Each request owns a new, hidden
// process. Neither lookup nor event delivery by application name is used for scripting.
#import <Cocoa/Cocoa.h>
#import <ApplicationServices/ApplicationServices.h>
#include <libproc.h>
#include <signal.h>

static NSAppleEventDescriptor *run_script(NSAppleEventDescriptor *address, NSString *script,
                                         NSTimeInterval timeout, NSError **error) {
    NSAppleEventDescriptor *event = [NSAppleEventDescriptor appleEventWithEventClass:'misc'
        eventID:'dosc' targetDescriptor:address returnID:kAutoGenerateReturnID
        transactionID:kAnyTransactionID];
    [event setParamDescriptor:[NSAppleEventDescriptor descriptorWithString:script] forKeyword:keyDirectObject];
    return [event sendEventWithOptions:NSAppleEventSendWaitForReply | NSAppleEventSendNeverInteract
                               timeout:timeout error:error];
}

// NSRunningApplication's lifecycle properties depend on main run loop updates. Check the
// kernel's launch timestamp before signalling, so a recycled PID is never a cleanup target.
static bool same_process(pid_t pid, const struct proc_bsdinfo *launched) {
    struct proc_bsdinfo current = {};
    return launched->pbi_start_tvsec &&
        proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &current, sizeof(current)) == sizeof(current) &&
        current.pbi_start_tvsec == launched->pbi_start_tvsec &&
        current.pbi_start_tvusec == launched->pbi_start_tvusec;
}

// Errors are returned to Rust for the portable JSX fallback.
int nf_ae_write_project(const char *token, const char *command, const char *artifact, const char *scriptError,
                        char *message, size_t capacity) {
    @autoreleasepool {
        NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
        NSString *identifier = @"com.adobe.AfterEffects.application";
        NSArray<NSURL *> *urls = [workspace URLsForApplicationsWithBundleIdentifier:identifier];
        NSURL *chosen = nil;
        NSString *version = @"";
        for (NSURL *url in urls) {
            NSString *v = [[NSBundle bundleWithURL:url] objectForInfoDictionaryKey:@"CFBundleShortVersionString"];
            if (v && (!chosen || [v compare:version options:NSNumericSearch] == NSOrderedDescending)) {
                chosen = url;
                version = v;
            }
        }
        NSString *reason = @"Adobe After Effects is unavailable.";
        if (chosen) {
            NSArray *prior = [NSRunningApplication runningApplicationsWithBundleIdentifier:identifier];
            NSError *launchError = nil;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            NSDictionary *configuration = @{
                NSWorkspaceLaunchConfigurationEnvironment: @{@"NEOFLASH_AE_SESSION": @(token)},
                // Version 27 otherwise opens its preferences migration/recovery dialog in the
                // hidden host. Earlier releases can require a window to initialize plugins.
                NSWorkspaceLaunchConfigurationArguments: version.integerValue >= 27 ? @[@"-m", @"-noui"] : @[@"-m"]
            };
            // The synchronous API provides the exact process even from an editor export on the main thread.
            NSRunningApplication *instance = [workspace launchApplicationAtURL:chosen
                options:NSWorkspaceLaunchNewInstance | NSWorkspaceLaunchWithoutActivation | NSWorkspaceLaunchAndHide
                configuration:configuration error:&launchError];
#pragma clang diagnostic pop
            bool owned = instance && instance.processIdentifier > 0;
            for (NSRunningApplication *old in prior) {
                if (old.processIdentifier == instance.processIdentifier) owned = false;
            }
            if (!owned) {
                reason = launchError.localizedDescription ?: @"Adobe did not start a separate application instance.";
            } else {
                pid_t pid = instance.processIdentifier;
                struct proc_bsdinfo launched = {};
                proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &launched, sizeof(launched));
                NSAppleEventDescriptor *address = [NSAppleEventDescriptor descriptorWithProcessIdentifier:instance.processIdentifier];
                NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:75.0];
                bool completed = false;
                reason = @"Adobe did not become ready for scripting.";
                while (!instance.terminated && [deadline timeIntervalSinceNow] > 0) {
                    // NSRunningApplication.finishedLaunching updates via the main run loop, which
                    // can be busy with the export. The harmless event is the readiness probe.
                    OSStatus permission = AEDeterminePermissionToAutomateTarget(address.aeDesc,'misc','dosc',false);
                    if (permission == errAEEventWouldRequireUserConsent && NSApp.running &&
                        [NSBundle.mainBundle objectForInfoDictionaryKey:@"NSAppleEventsUsageDescription"]) {
                        // Only an interactive desktop export may ask for first-time consent.
                        permission = AEDeterminePermissionToAutomateTarget(address.aeDesc,'misc','dosc',true);
                    }
                    if (permission == errAEEventNotPermitted || permission == errAEEventWouldRequireUserConsent) {
                        reason = @"macOS has not granted NeoFlash access to After Effects in Privacy & Security > Automation.";
                        break;
                    }
                    if (permission == noErr) {
                        NSError *err = nil;
                        NSAppleEventDescriptor *ready = run_script(address, @"void 0", 2.0, &err);
                        if (ready && !err && ![ready paramDescriptorForKeyword:keyErrorNumber]) {
                            err = nil;
                            NSAppleEventDescriptor *reply = run_script(address, @(command), 180.0, &err);
                            if (!reply || err || [reply paramDescriptorForKeyword:keyErrorNumber]) {
                                reason = err.localizedDescription ?: @"Adobe rejected the project script.";
                                break;
                            }
                            // Some versions acknowledge DoScript before ExtendScript has started.
                            // Only a saved artifact (or an explicit builder error) completes a request.
                            // Retrying cannot append to a project because the script requires an empty one.
                            NSFileManager *files = NSFileManager.defaultManager;
                            NSDictionary *attributes = [files attributesOfItemAtPath:@(artifact) error:NULL];
                            if ([attributes[NSFileSize] unsignedLongLongValue] > 0 ||
                                [files fileExistsAtPath:@(scriptError)]) {
                                completed = true;
                                break;
                            }
                        }
                    }
                    [NSThread sleepForTimeInterval:0.5];
                }
                // A startup dialog, hung plugin or denied event must not leave an orphan host.
                // This is the process returned by our new-instance launch, never a reused application.
                if (completed) {
                    [instance terminate];
                    NSDate *quitDeadline = [NSDate dateWithTimeIntervalSinceNow:8.0];
                    while (same_process(pid, &launched) && [quitDeadline timeIntervalSinceNow] > 0) {
                        [NSThread sleepForTimeInterval:0.1];
                    }
                }
                if (same_process(pid, &launched)) {
                    kill(pid, SIGKILL);
                } else if (!launched.pbi_start_tvsec && !instance.terminated) {
                    [instance forceTerminate];
                }
                if (completed) return 0;
            }
        }
        snprintf(message,capacity,"%s",reason.UTF8String);
        return 1;
    }
}
