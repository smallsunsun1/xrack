#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

static BOOL OpenContextMenu(id trigger) {
  for (UIAccessibilityCustomAction *action in [trigger accessibilityCustomActions]) {
    if ([action.name isEqualToString:@"Show context menu"] && action.actionHandler != nil) {
      return action.actionHandler(action);
    }
  }
  return NO;
}

@interface XRackMenuAccessibilityTest : XCTestCase
@end

@implementation XRackMenuAccessibilityTest

- (id)waitForLabel:(NSString *)label {
  NSPredicate *predicate = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return AccessibleElement(WindowAccessibilityHost(), label) != nil;
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:predicate object:nil];
  XCTAssertEqual([XCTWaiter waitForExpectations:@[expectation] timeout:10], XCTWaiterResultCompleted,
                 @"Expected %@", label);
  return AccessibleElement(WindowAccessibilityHost(), label);
}

- (void)testPagesRootAndNestedMenusWithoutActivatingCommands {
  self.continueAfterFailure = NO;
  XCTAssertTrue(OpenContextMenu([self waitForLabel:@"Review commands"]));
  id first = [self waitForLabel:@"Review 01"];
  UIView *host = WindowAccessibilityHost();
  XCTAssertNil(AccessibleElement(host, @"Apply review"));
  XCTAssertFalse([first accessibilityScroll:UIAccessibilityScrollDirectionDown]);
  XCTAssertTrue([first accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  id last = [self waitForLabel:@"Apply review"];
  XCTAssertNil(AccessibleElement(host, @"Review 01"));
  XCTAssertFalse([first accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  XCTAssertFalse([last accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  XCTAssertTrue([last accessibilityScroll:UIAccessibilityScrollDirectionDown]);

  id archive = [self waitForLabel:@"Archive filters"];
  XCTAssertNil(AccessibleElement(host, @"Apply review"));
  XCTAssertTrue([archive accessibilityActivate]);
  id archiveFirst = [self waitForLabel:@"Archive 01"];
  XCTAssertNil(AccessibleElement(host, @"Apply archive filter"));
  XCTAssertTrue([archiveFirst accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  id archiveLast = [self waitForLabel:@"Apply archive filter"];
  XCTAssertEqual(archive, AccessibleElement(host, @"Archive filters"));
  XCTAssertNil(AccessibleElement(host, @"Archive 01"));
  XCTAssertFalse([archiveLast accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  XCTAssertTrue([archiveLast accessibilityActivate]);
  [self waitForLabel:@"Filter applied"];
  XCTAssertNil(AccessibleElement(host, @"Archive filters"));
  XCTAssertNil(AccessibleElement(host, @"Review applied"));
}

- (void)testChoicePagesPastDisabledOptionsAndRejectsStaleElements {
  self.continueAfterFailure = NO;
  id trigger = [self waitForLabel:@"Paging interval"];
  XCTAssertTrue([trigger accessibilityActivate]);
  id first = [self waitForLabel:@"Daily"];
  XCTAssertFalse([first accessibilityScroll:UIAccessibilityScrollDirectionDown]);
  XCTAssertTrue([first accessibilityScroll:UIAccessibilityScrollDirectionUp]);

  id disabled = [self waitForLabel:@"Paused A"];
  XCTAssertTrue(([disabled accessibilityTraits] & UIAccessibilityTraitNotEnabled) != 0);
  XCTAssertFalse([disabled accessibilityActivate]);
  XCTAssertFalse([first accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  XCTAssertTrue([disabled accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  id last = [self waitForLabel:@"Annual"];
  XCTAssertFalse([last accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  XCTAssertTrue([last accessibilityScroll:UIAccessibilityScrollDirectionDown]);

  id previous = [self waitForLabel:@"Paused C"];
  XCTAssertTrue([previous accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  id reopenedRow = [self waitForLabel:@"Annual"];
  XCTAssertFalse([last accessibilityActivate]);
  [self waitForLabel:@"Interval: Daily"];
  XCTAssertTrue([reopenedRow accessibilityActivate]);
  [self waitForLabel:@"Interval: Annual"];
  XCTAssertNil(AccessibleElement(WindowAccessibilityHost(), @"Annual"));

  XCTAssertTrue([trigger accessibilityActivate]);
  id current = [self waitForLabel:@"Annual"];
  XCTAssertFalse([reopenedRow accessibilityActivate]);
  XCTAssertTrue([current accessibilityActivate]);
}

@end
