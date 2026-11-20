#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static UIAccessibilityCustomAction *ActionNamed(id element, NSString *name) {
  for (UIAccessibilityCustomAction *action in [element accessibilityCustomActions]) {
    if ([action.name isEqualToString:name]) {
      return action;
    }
  }
  return nil;
}

static BOOL Invoke(UIAccessibilityCustomAction *action) {
  UIAccessibilityCustomActionHandler handler = action.actionHandler;
  return handler != nil && handler(action);
}

static void CollectLabels(id container, NSMutableArray<NSString *> *labels) {
  for (id child in [container accessibilityElements]) {
    if ([child isAccessibilityElement]) {
      NSString *label = [child accessibilityLabel];
      if (label != nil) {
        [labels addObject:label];
      }
    } else {
      CollectLabels(child, labels);
    }
  }
}

@interface XRackSwipeAccessibilityTest : XCTestCase
@end

@implementation XRackSwipeAccessibilityTest

- (void)waitUntil:(BOOL (^)(void))condition {
  NSPredicate *predicate = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:predicate object:nil];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:10], XCTWaiterResultCompleted);
}

- (void)assertReadingOrderInHost:(UIView *)host action:(NSString *)action {
  NSMutableArray<NSString *> *labels = [NSMutableArray array];
  CollectLabels(host, labels);
  NSUInteger row = [labels indexOfObject:@"Review document"];
  NSUInteger button = [labels indexOfObject:action];
  NSUInteger nextRow = [labels indexOfObject:@"Next document"];
  XCTAssertNotEqual(row, NSNotFound);
  XCTAssertEqual(button, row + 1);
  XCTAssertLessThan(button, nextRow);
  XCTAssertNotEqual(nextRow, NSNotFound);
}

- (void)testBothSwipeSidesAreReachableFromTheSameRow {
  self.continueAfterFailure = NO;
  [self waitUntil:^BOOL {
    return ActionNamed(AccessibleElement(WindowAccessibilityHost(), @"Review document"),
                       @"Show left actions") != nil;
  }];
  UIView *host = WindowAccessibilityHost();
  id row = AccessibleElement(host, @"Review document");
  id next = AccessibleElement(host, @"Next document");
  XCTAssertTrue([row isAccessibilityElement]);
  XCTAssertNotNil(ActionNamed(row, @"Show right actions"));
  XCTAssertNil(ActionNamed(next, @"Show left actions"));
  XCTAssertNotNil(ActionNamed(next, @"Show right actions"));
  XCTAssertNil(AccessibleElement(host, @"Pin document"));
  XCTAssertNil(AccessibleElement(host, @"Archive document"));

  UIAccessibilityCustomAction *left = ActionNamed(row, @"Show left actions");
  XCTAssertTrue(Invoke(left));
  [self waitUntil:^BOOL { return AccessibleElement(host, @"Pin document") != nil; }];
  XCTAssertEqual(row, AccessibleElement(host, @"Review document"));
  XCTAssertFalse(Invoke(left));
  XCTAssertNil(AccessibleElement(host, @"Archive document"));
  [self assertReadingOrderInHost:host action:@"Pin document"];

  XCTAssertTrue(Invoke(ActionNamed(row, @"Show right actions")));
  [self waitUntil:^BOOL { return AccessibleElement(host, @"Archive document") != nil; }];
  XCTAssertEqual(row, AccessibleElement(host, @"Review document"));
  XCTAssertNil(AccessibleElement(host, @"Pin document"));
  XCTAssertNotNil(AccessibleElement(host, @"Ready"));
  [self assertReadingOrderInHost:host action:@"Archive document"];

  XCTAssertTrue(Invoke(ActionNamed(row, @"Hide swipe actions")));
  [self waitUntil:^BOOL {
    return AccessibleElement(host, @"Archive document") == nil &&
           ActionNamed(row, @"Show right actions") != nil;
  }];
  XCTAssertNil(ActionNamed(row, @"Hide swipe actions"));
  XCTAssertTrue(Invoke(ActionNamed(row, @"Show right actions")));
  [self waitUntil:^BOOL { return AccessibleElement(host, @"Archive document") != nil; }];
  XCTAssertTrue([AccessibleElement(host, @"Archive document") accessibilityActivate]);
  [self waitUntil:^BOOL { return AccessibleElement(host, @"Archived") != nil; }];
  XCTAssertNil(AccessibleElement(host, @"Archive document"));
}

@end
