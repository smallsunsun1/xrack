#pragma once

#import <UIKit/UIKit.h>

static UIView *AccessibilityHost(UIView *view) {
  if ([view isKindOfClass:NSClassFromString(@"XRackAccessibilityView")]) {
    return view;
  }
  for (UIView *child in view.subviews) {
    UIView *host = AccessibilityHost(child);
    if (host != nil) {
      return host;
    }
  }
  return nil;
}

static UIView *WindowAccessibilityHost(void) {
  for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
    if (![scene isKindOfClass:UIWindowScene.class]) {
      continue;
    }
    for (UIWindow *window in ((UIWindowScene *)scene).windows) {
      UIView *host = AccessibilityHost(window);
      if (host != nil) {
        return host;
      }
    }
  }
  return nil;
}

static id FindAccessibleElement(id container, BOOL (^matches)(id)) {
  for (id child in [container accessibilityElements]) {
    if (matches(child)) {
      return child;
    }
    id descendant = FindAccessibleElement(child, matches);
    if (descendant != nil) {
      return descendant;
    }
  }
  return nil;
}

static id AccessibleElement(id container, NSString *label) {
  return FindAccessibleElement(container, ^BOOL(id element) {
    return [[element accessibilityLabel] isEqualToString:label];
  });
}
