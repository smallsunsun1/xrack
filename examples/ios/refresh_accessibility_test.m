#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static UIAccessibilityCustomAction *RefreshAction(id element) {
  for (UIAccessibilityCustomAction *action in [element accessibilityCustomActions]) {
    if ([action.name isEqualToString:@"Refresh"]) {
      return action;
    }
  }
  return nil;
}

static BOOL Invoke(UIAccessibilityCustomAction *action) {
  UIAccessibilityCustomActionHandler handler = action.actionHandler;
  return handler != nil && handler(action);
}

@interface XRackRefreshAccessibilityTest : XCTestCase
@end

@implementation XRackRefreshAccessibilityTest

- (void)waitUntil:(BOOL (^)(void))condition {
  NSPredicate *predicate = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:predicate object:nil];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:10], XCTWaiterResultCompleted);
}

- (void)testRefreshIsAvailableOnTheRowsAndSuspendedDuringLoading {
  self.continueAfterFailure = NO;
  [self waitUntil:^BOOL {
    return RefreshAction(AccessibleElement(WindowAccessibilityHost(), @"Pump pressure")) != nil;
  }];

  UIView *host = WindowAccessibilityHost();
  id pump = AccessibleElement(host, @"Pump pressure");
  id door = AccessibleElement(host, @"Door interlock");
  XCTAssertTrue([pump isAccessibilityElement]);
  XCTAssertTrue([door isAccessibilityElement]);
  UIAccessibilityCustomAction *first = RefreshAction(pump);
  UIAccessibilityCustomAction *second = RefreshAction(door);
  XCTAssertNotNil(second);
  XCTAssertTrue(Invoke(first));

  [self waitUntil:^BOOL {
    return AccessibleElement(host, @"Refreshing…") != nil && RefreshAction(pump) == nil;
  }];
  XCTAssertNil(RefreshAction(door));
  XCTAssertFalse(Invoke(first));
  XCTAssertFalse(Invoke(second));
  XCTAssertEqual(pump, AccessibleElement(host, @"Pump pressure"));
  XCTAssertEqual(door, AccessibleElement(host, @"Door interlock"));

  [self waitUntil:^BOOL {
    return AccessibleElement(host, @"Completed refreshes: 1") != nil && RefreshAction(door) != nil;
  }];
  XCTAssertTrue(Invoke(RefreshAction(door)));
  [self waitUntil:^BOOL {
    return AccessibleElement(host, @"Completed refreshes: 2") != nil;
  }];
  XCTAssertEqual(pump, AccessibleElement(host, @"Pump pressure"));
}

@end
