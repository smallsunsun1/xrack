#import <UIKit/UIKit.h>

@interface ClipboardFixtureDelegate : UIResponder<UIApplicationDelegate>
@property(nonatomic, strong) UIWindow *window;
@end

@implementation ClipboardFixtureDelegate

- (BOOL)application:(UIApplication *)application
    didFinishLaunchingWithOptions:(NSDictionary<UIApplicationLaunchOptionsKey, id> *)options {
  UIViewController *controller = [[UIViewController alloc] init];
  controller.view.backgroundColor = UIColor.systemBackgroundColor;

  UIButton *copy = [UIButton buttonWithType:UIButtonTypeSystem];
  [copy setTitle:@"Copy fixture text" forState:UIControlStateNormal];
  [copy addTarget:self action:@selector(copyText:) forControlEvents:UIControlEventTouchUpInside];
  copy.translatesAutoresizingMaskIntoConstraints = NO;
  [controller.view addSubview:copy];
  [NSLayoutConstraint activateConstraints:@[
    [copy.centerXAnchor constraintEqualToAnchor:controller.view.centerXAnchor],
    [copy.centerYAnchor constraintEqualToAnchor:controller.view.centerYAnchor],
    [copy.heightAnchor constraintGreaterThanOrEqualToConstant:44],
  ]];

  self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
  self.window.rootViewController = controller;
  [self.window makeKeyAndVisible];
  return YES;
}

- (void)copyText:(UIButton *)sender {
  UIPasteboard.generalPasteboard.string = @"clipboard sample";
  [sender setTitle:@"Fixture text copied" forState:UIControlStateNormal];
}

@end

int main(int argc, char *argv[]) {
  @autoreleasepool {
    return UIApplicationMain(argc, argv, nil, NSStringFromClass(ClipboardFixtureDelegate.class));
  }
}
