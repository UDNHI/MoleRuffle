#import <Cocoa/Cocoa.h>
#include <signal.h>
#include <unistd.h>

@interface Launcher : NSObject <NSApplicationDelegate>
@property NSWindow *window;
@property NSTextField *status;
@property NSProgressIndicator *spinner;
@property NSTask *task;
@property NSString *dataDir;
@property NSString *logPath;
@property BOOL quitting;
@end
@implementation Launcher
- (void)applicationDidFinishLaunching:(NSNotification *)note {
    NSString *override=NSProcessInfo.processInfo.environment[@"MOLERUFFLE_TEST_DATA_DIR"];
    self.dataDir=override ?: [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support/MoleRuffle-Taiwan-Test"];
    NSError *error=nil;
    if(![NSFileManager.defaultManager createDirectoryAtPath:self.dataDir withIntermediateDirectories:YES attributes:nil error:&error]) {
        NSAlert *a=[NSAlert new];a.messageText=@"無法建立遊戲資料夾";a.informativeText=error.localizedDescription;[a runModal];[NSApp terminate:nil];return;
    }
    NSMenu *menu=[NSMenu new]; NSMenuItem *root=[NSMenuItem new];[menu addItem:root];
    NSMenu *appMenu=[NSMenu new];root.submenu=appMenu;
    [appMenu addItemWithTitle:@"關於 MoleRuffle 台版" action:@selector(orderFrontStandardAboutPanel:) keyEquivalent:@""];
    [appMenu addItem:NSMenuItem.separatorItem];
    [appMenu addItemWithTitle:@"結束遊戲" action:@selector(terminate:) keyEquivalent:@"q"];NSApp.mainMenu=menu;
    self.window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,440,260) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    self.window.title=@"MoleRuffle 台版";
    NSImageView *icon=[[NSImageView alloc] initWithFrame:NSMakeRect(184,158,72,72)];icon.image=NSApp.applicationIconImage;[self.window.contentView addSubview:icon];
    NSTextField *title=[NSTextField labelWithString:@"準備回到莊園"];title.frame=NSMakeRect(30,119,380,28);title.alignment=NSTextAlignmentCenter;title.font=[NSFont boldSystemFontOfSize:21];[self.window.contentView addSubview:title];
    self.status=[NSTextField labelWithString:@""];self.status.frame=NSMakeRect(24,80,392,28);self.status.alignment=NSTextAlignmentCenter;self.status.font=[NSFont systemFontOfSize:13];[self.window.contentView addSubview:self.status];
    self.spinner=[[NSProgressIndicator alloc] initWithFrame:NSMakeRect(60,53,320,10)];self.spinner.style=NSProgressIndicatorStyleBar;self.spinner.indeterminate=YES;[self.window.contentView addSubview:self.spinner];
    NSTextField *noteField=[NSTextField labelWithString:[NSString stringWithFormat:@"v%@ · 摩爾莊園非官方客戶端", [NSBundle.mainBundle objectForInfoDictionaryKey:@"CFBundleShortVersionString"] ?: @"開發版"]];noteField.frame=NSMakeRect(20,15,400,18);noteField.alignment=NSTextAlignmentCenter;noteField.textColor=NSColor.secondaryLabelColor;noteField.font=[NSFont systemFontOfSize:11];[self.window.contentView addSubview:noteField];
    [self.window center];[self prepare];
}
- (NSMutableDictionary *)environment {
    NSMutableDictionary *env=[NSProcessInfo.processInfo.environment mutableCopy];
    env[@"MOLERUFFLE_DATA_DIR"]=self.dataDir;env[@"MOLERUFFLE_TEST_DATA_DIR"]=self.dataDir;
    env[@"MOLERUFFLE_BANK_SWF"]=[self.dataDir stringByAppendingPathComponent:@"bank-patched.swf"];
    env[@"RUST_LOG"]=@"warn,moleruffle=info,moleruffle_core=info,ruffle_core::avm2=info";
    // Legacy score validation uses the local calendar day. Keep it in Taiwan.
    env[@"TZ"]=@"Asia/Taipei";
    // wgpu debug locks call Backtrace::capture on routine GPU operations.
    // Disable library captures while retaining panic backtraces.
    env[@"RUST_LIB_BACKTRACE"]=@"0";
    env[@"RUST_BACKTRACE"]=@"1";return env;
}
- (void)prepare {
    [self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];
    BOOL first=![NSFileManager.defaultManager fileExistsAtPath:[self.dataDir stringByAppendingPathComponent:@"bank-source.swf"]];
    self.status.stringValue=first ? @"首次啟動：正在下載必要資源，請稍候…" : @"正在檢查遊戲資源…";
    [self.spinner startAnimation:nil];
    self.task=[NSTask new];self.task.executableURL=[NSURL fileURLWithPath:@"/bin/sh"];
    self.task.arguments=@[[NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"launch.sh"]];
    NSMutableDictionary *env=[self environment];env[@"MOLERUFFLE_PREPARE_ONLY"]=@"1";self.task.environment=env;
    self.logPath=[self.dataDir stringByAppendingPathComponent:@"launcher.log"];
    [NSFileManager.defaultManager createFileAtPath:self.logPath contents:nil attributes:nil];
    NSFileHandle *log=[NSFileHandle fileHandleForWritingAtPath:self.logPath];self.task.standardOutput=log;self.task.standardError=log;
    __weak Launcher *weak=self;
    self.task.terminationHandler=^(NSTask *t){dispatch_async(dispatch_get_main_queue(),^{
        [log closeFile];if(weak.quitting)return;
        if(t.terminationStatus==0)[weak launchGame];else [weak showError:@"暫時無法準備遊戲" message:@"請確認網路可連線後重試。如果持續失敗，請用「開啟日誌」將紀錄附在問題回報中。"];
    });};
    NSError *error=nil;if(![self.task launchAndReturnError:&error]){[log closeFile];[self showError:@"無法啟動準備工具" message:error.localizedDescription];}
}
- (void)launchGame {
    self.status.stringValue=@"資源準備完成，正在開啟遊戲…";
    NSDateFormatter *date=[NSDateFormatter new];date.dateFormat=@"yyyyMMdd-HHmmss";
    self.logPath=[self.dataDir stringByAppendingPathComponent:[NSString stringWithFormat:@"game-%@.log",[date stringFromDate:NSDate.date]]];
    [NSFileManager.defaultManager createFileAtPath:self.logPath contents:nil attributes:nil];
    NSFileHandle *log=[NSFileHandle fileHandleForWritingAtPath:self.logPath];
    self.task=[NSTask new];self.task.executableURL=[NSURL fileURLWithPath:[NSBundle.mainBundle.executablePath.stringByDeletingLastPathComponent stringByAppendingPathComponent:@"moleruffle"]];
    NSMutableDictionary *env=[self environment];[env removeObjectForKey:@"MOLERUFFLE_PREPARE_ONLY"];self.task.environment=env;
    self.task.standardOutput=log;self.task.standardError=log;
    __weak Launcher *weak=self;
    self.task.terminationHandler=^(NSTask *t){dispatch_async(dispatch_get_main_queue(),^{
        BOOL signalExit=t.terminationReason==NSTaskTerminationReasonUncaughtSignal;
        NSString *result=[NSString stringWithFormat:@"\n[launcher] Game ended: reason=%@ status=%d\n",signalExit ? @"signal" : @"exit",t.terminationStatus];
        [log writeData:[result dataUsingEncoding:NSUTF8StringEncoding]];
        [log closeFile];if(weak.quitting)return;
        if(t.terminationStatus==0)[NSApp terminate:nil];
        else {
            NSString *detail=signalExit ? [NSString stringWithFormat:@"程式被訊號 %d 終止。",t.terminationStatus] : [NSString stringWithFormat:@"程式退出碼：%d。",t.terminationStatus];
            [weak showError:@"遊戲意外結束" message:[detail stringByAppendingString:@"可以重試啟動；若再次發生，請用「開啟日誌」找到這次的紀錄。"]];
        }
    });};
    NSError *error=nil;if(![self.task launchAndReturnError:&error]){[log closeFile];[self showError:@"無法開啟遊戲" message:error.localizedDescription];return;}
    [self.spinner stopAnimation:nil];[self.window orderOut:nil];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
}
- (void)showError:(NSString *)title message:(NSString *)message {
    [self.spinner stopAnimation:nil];[NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];[NSApp activateIgnoringOtherApps:YES];
    NSAlert *alert=[NSAlert new];alert.messageText=title;alert.informativeText=message;alert.alertStyle=NSAlertStyleWarning;
    [alert addButtonWithTitle:@"重試"];[alert addButtonWithTitle:@"開啟日誌"];[alert addButtonWithTitle:@"關閉"];
    NSModalResponse choice=[alert runModal];
    if(choice==NSAlertFirstButtonReturn)[self prepare];
    else if(choice==NSAlertSecondButtonReturn){[NSWorkspace.sharedWorkspace openURL:[NSURL fileURLWithPath:self.dataDir]];dispatch_async(dispatch_get_main_queue(),^{[self showError:title message:message];});}
    else [NSApp terminate:nil];
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    self.quitting=YES;if(self.task.running)[self.task terminate];return NSTerminateNow;
}
@end
int main(int argc,char **argv) {
    @autoreleasepool { NSApplication *app=NSApplication.sharedApplication;[app setActivationPolicy:NSApplicationActivationPolicyRegular];__attribute__((objc_precise_lifetime)) Launcher *delegate=[Launcher new];app.delegate=delegate;[app run]; }
    return 0;
}
