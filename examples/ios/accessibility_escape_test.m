#import <UIKit/UIKit.h>
#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static UIView *EscapeContainer(id element) {
  while ([element isKindOfClass:UIAccessibilityElement.class]) {
    element = [element accessibilityContainer];
  }
  return [element isKindOfClass:UIView.class] ? element : nil;
}

@interface XRackAccessibilityEscapeTest : XCTestCase
@end

@implementation XRackAccessibilityEscapeTest

- (void)testUIKitEscapeRejectsOneDialogAndRevealsOpener {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    UIView *host = WindowAccessibilityHost();
    return host != nil && AccessibleElement(host, @"Open dialog") != nil;
  }];
  [self expectationForPredicate:ready evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:20 handler:nil];

  UIView *host = WindowAccessibilityHost();
  XCTAssertNotNil(host);
  XCTAssertFalse([host accessibilityPerformEscape]);
  id opener = AccessibleElement(host, @"Open dialog");
  XCTAssertTrue(CGRectContainsRect(host.window.frame, [opener accessibilityFrame]));
  XCTAssertTrue([opener accessibilityActivate]);

  NSPredicate *open = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Keep the current values?") != nil;
  }];
  [self expectationForPredicate:open evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertNil(AccessibleElement(host, @"Open dialog"));
  UIView *container = EscapeContainer(AccessibleElement(host, @"Keep the current values?"));
  XCTAssertEqual(container, host);
  XCTAssertTrue([container accessibilityPerformEscape]);
  XCTAssertFalse([host accessibilityPerformEscape]);

  NSPredicate *closed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Rejected 1") != nil &&
           AccessibleElement(view, @"Open dialog") != nil;
  }];
  [self expectationForPredicate:closed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertNil(AccessibleElement(host, @"Keep the current values?"));
  XCTAssertNotNil(AccessibleElement(host, @"Accepted 0"));
  XCTAssertFalse([host accessibilityPerformEscape]);
}

- (void)testUIKitEscapeClosesAChoiceWithoutCommittingIt {
  BOOL (^hasNormalValue)(id) = ^BOOL(id element) {
    return [[element accessibilityValue] isEqualToString:@"Normal"];
  };
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    UIView *host = WindowAccessibilityHost();
    return host != nil && FindAccessibleElement(host, hasNormalValue) != nil;
  }];
  [self expectationForPredicate:ready evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:20 handler:nil];

  UIView *host = WindowAccessibilityHost();
  XCTAssertTrue([FindAccessibleElement(host, hasNormalValue) accessibilityActivate]);
  NSPredicate *expanded = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Urgent") != nil;
  }];
  [self expectationForPredicate:expanded evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  UIView *container = EscapeContainer(AccessibleElement(host, @"Urgent"));
  XCTAssertEqual(container, host);
  XCTAssertTrue([container accessibilityPerformEscape]);

  NSPredicate *closed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Urgent") == nil &&
           AccessibleElement(view, @"Priority Normal") != nil;
  }];
  [self expectationForPredicate:closed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse([host accessibilityPerformEscape]);
}

@end
