#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>

#include <cstdint>

using WorkbenchNotificationCallback = void (*)(int32_t event, int64_t value);
static constexpr int32_t Authorization = 0;
static constexpr int32_t Activation = 1;
static constexpr int32_t DeliveryFailed = 2;
static NSString *const Category = @"xrack-open-conversation";
static NSString *const OpenConversation = @"open-conversation";

@interface XRackWorkbenchNotifications : NSObject <UNUserNotificationCenterDelegate>
@property(nonatomic, copy) NSString *launch;
@property(nonatomic, assign) WorkbenchNotificationCallback callback;
@end

@implementation XRackWorkbenchNotifications
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))complete {
    complete(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList);
}

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
didReceiveNotificationResponse:(UNNotificationResponse *)response
         withCompletionHandler:(void (^)(void))complete {
    NSDictionary *info = response.notification.request.content.userInfo;
    NSString *action = response.actionIdentifier;
    if ([info[@"launch"] isEqual:self.launch] &&
        ([action isEqual:OpenConversation] ||
         [action isEqual:UNNotificationDefaultActionIdentifier])) {
        NSNumber *conversation = info[@"conversation"];
        if ([conversation isKindOfClass:[NSNumber class]]) {
            self.callback(Activation, conversation.longLongValue);
        }
    }
    complete();
}
@end

// AppKit's event loop owns initialization and posting. The delegate can receive
// responses on a system queue, so its callback forwards scalar values only.
static XRackWorkbenchNotifications *delegate;

extern "C" bool xr_workbench_notifications_available() {
    return [NSBundle.mainBundle.bundleIdentifier isEqualToString:@"dev.xrack.agent-workbench"];
}

static void report_authorization(XRackWorkbenchNotifications *target) {
    [UNUserNotificationCenter.currentNotificationCenter
        getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
            int64_t value = 0;
            switch (settings.authorizationStatus) {
            case UNAuthorizationStatusAuthorized:
            case UNAuthorizationStatusProvisional:
                value = 1;
                break;
            case UNAuthorizationStatusDenied:
                value = 2;
                break;
            default:
                break;
            }
            target.callback(Authorization, value);
        }];
}

extern "C" void xr_workbench_notifications_start(WorkbenchNotificationCallback callback) {
    if (!xr_workbench_notifications_available()) {
        return;
    }
    if (!delegate) {
        delegate = [XRackWorkbenchNotifications new];
        delegate.launch = NSUUID.UUID.UUIDString;
        delegate.callback = callback;
        UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
        center.delegate = delegate;
        UNNotificationAction *open = [UNNotificationAction
            actionWithIdentifier:OpenConversation
                           title:@"打开会话"
                         options:UNNotificationActionOptionForeground];
        UNNotificationCategory *category = [UNNotificationCategory
            categoryWithIdentifier:Category
                           actions:@[open]
                 intentIdentifiers:@[]
                           options:UNNotificationCategoryOptionNone];
        [center setNotificationCategories:[NSSet setWithObject:category]];
    }
    report_authorization(delegate);
}

extern "C" void xr_workbench_notifications_request() {
    XRackWorkbenchNotifications *target = delegate;
    if (target) {
        [UNUserNotificationCenter.currentNotificationCenter
            requestAuthorizationWithOptions:UNAuthorizationOptionAlert
                          completionHandler:^(BOOL granted, NSError *error) {
                              if (error) {
                                  target.callback(DeliveryFailed, 0);
                              }
                              report_authorization(target);
                          }];
    }
}

extern "C" void xr_workbench_notifications_post(int64_t conversation, const char *title,
                                                  const char *body, bool question) {
    XRackWorkbenchNotifications *target = delegate;
    if (!target) {
        return;
    }
    UNMutableNotificationContent *content = [UNMutableNotificationContent new];
    content.title = [NSString stringWithUTF8String:title];
    content.body = [NSString stringWithUTF8String:body];
    content.categoryIdentifier = Category;
    content.userInfo = @{@"launch": target.launch, @"conversation": @(conversation)};
    NSString *identifier = [NSString stringWithFormat:@"%@-%lld-%@", target.launch,
                            (long long)conversation, question ? @"question" : @"complete"];
    UNNotificationRequest *request = [UNNotificationRequest
        requestWithIdentifier:identifier content:content trigger:nil];
    [UNUserNotificationCenter.currentNotificationCenter
        addNotificationRequest:request withCompletionHandler:^(NSError *error) {
            if (error) {
                target.callback(DeliveryFailed, 0);
            }
        }];
}
