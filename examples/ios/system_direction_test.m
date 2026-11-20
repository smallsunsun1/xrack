#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static CGFloat Position(UIView *host, NSString *label) {
  id element = AccessibleElement(host, label);
  return element == nil ? NAN : CGRectGetMidX([element accessibilityFrame]);
}

@interface XRackSystemDirectionTest : XCTestCase
@end

@implementation XRackSystemDirectionTest

- (void)await:(BOOL (^)(void))condition {
  NSPredicate *predicate = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  [self expectationForPredicate:predicate evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];
}

- (void)testUIKitMirrorsInheritedControlsAndPreservesExplicitDirectionAndState {
  self.continueAfterFailure = NO;
  [self await:^BOOL {
    return AccessibleElement(WindowAccessibilityHost(), @"Review") != nil;
  }];
  UIView *host = WindowAccessibilityHost();
  UIView *view = host.superview;
  view.traitOverrides.layoutDirection = UITraitEnvironmentLayoutDirectionLeftToRight;
  [self await:^BOOL {
    return Position(host, @"Review") < Position(host, @"Complete");
  }];
  XCTAssertLessThan(Position(host, @"Batch number"), Position(host, @"Check digit"));
  XCTAssertTrue([AccessibleElement(host, @"Review") accessibilityActivate]);
  [self await:^BOOL {
    return AccessibleElement(host, @"Selected: Review") != nil;
  }];

  view.traitOverrides.layoutDirection = UITraitEnvironmentLayoutDirectionRightToLeft;
  [self await:^BOOL {
    return Position(host, @"Review") > Position(host, @"Complete");
  }];
  XCTAssertLessThan(Position(host, @"Batch number"), Position(host, @"Check digit"));
  XCTAssertNotNil(AccessibleElement(host, @"Selected: Review"));
  id slider = AccessibleElement(host, @"Inspection progress");
  XCTAssertEqualWithAccuracy([[slider accessibilityValue] doubleValue], 25.0, 0.001);
  [slider accessibilityIncrement];
  [self await:^BOOL {
    return [[AccessibleElement(host, @"Inspection progress") accessibilityValue] doubleValue] == 30.0;
  }];

  view.traitOverrides.layoutDirection = UITraitEnvironmentLayoutDirectionLeftToRight;
  [self await:^BOOL {
    return Position(host, @"Review") < Position(host, @"Complete");
  }];
  XCTAssertEqualWithAccuracy(
      [[AccessibleElement(host, @"Inspection progress") accessibilityValue] doubleValue], 30.0, 0.001);
  XCTAssertNotNil(AccessibleElement(host, @"Selected: Review"));
}

@end
