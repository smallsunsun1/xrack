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

static UITextField *ActiveTextField(UIView *view) {
  if ([view isKindOfClass:UITextField.class] && [view isFirstResponder]) {
    return (UITextField *)view;
  }
  for (UIView *child in view.subviews) {
    UITextField *field = ActiveTextField(child);
    if (field != nil) {
      return field;
    }
  }
  return nil;
}

@interface XRackNativeActionsTest : XCTestCase
@end

@implementation XRackNativeActionsTest

- (UIView *)waitForHost {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    UIView *host = WindowAccessibilityHost();
    return host != nil && AccessibleElement(host, @"Review options") != nil;
  }];
  [self expectationForPredicate:ready evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:20 handler:nil];
  UIView *host = WindowAccessibilityHost();
  return AccessibleElement(host, @"Review options") != nil ? host : nil;
}

- (void)waitForLabel:(NSString *)label inHost:(UIView *)host {
  NSPredicate *visible = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, label) != nil;
  }];
  [self expectationForPredicate:visible evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testUIKitOpensTheContextMenuAndRevokesAnOldAction {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id button = AccessibleElement(host, @"Review options");
  UIAccessibilityCustomAction *open = ActionNamed(button, @"Show context menu");
  XCTAssertTrue(Invoke(open));
  [self waitForLabel:@"Record review" inHost:host];
  XCTAssertFalse(Invoke(open));
  XCTAssertTrue([AccessibleElement(host, @"Record review") accessibilityActivate]);
  [self waitForLabel:@"Reviews 1" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Record review"));
}

- (void)testUIKitMovesTheSameTableRowAndUpdatesItsBoundaryActions {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id row = FindAccessibleElement(host, ^BOOL(id element) {
    return ActionNamed(element, @"Move row up") != nil &&
           ActionNamed(element, @"Move row down") != nil;
  });
  XCTAssertNotNil(row);
  if (row == nil) {
    return;
  }
  UIAccessibilityCustomAction *moveUp = ActionNamed(row, @"Move row up");
  NSString *rowIdentifier = [row accessibilityIdentifier];
  XCTAssertNotNil(rowIdentifier);
  XCTAssertTrue(Invoke(moveUp));
  [self waitForLabel:@"Moved row 2 to 1" inHost:host];
  XCTAssertFalse(Invoke(moveUp));

  id movedRow = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityIdentifier] isEqualToString:rowIdentifier];
  });
  XCTAssertNotNil(movedRow);
  XCTAssertNil(ActionNamed(movedRow, @"Move row up"));
  XCTAssertTrue(Invoke(ActionNamed(movedRow, @"Move row down")));
  [self waitForLabel:@"Moved row 1 to 2" inHost:host];
}

- (void)waitForSortValue:(NSString *)value
                  order:(NSArray<NSString *> *)order
                 inHost:(UIView *)host {
  NSPredicate *sorted = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id header = AccessibleElement(view, @"Instrument");
    if (![[header accessibilityValue] isEqualToString:value]) {
      return NO;
    }
    CGFloat lastY = -CGFLOAT_MAX;
    for (NSString *name in order) {
      id cell = AccessibleElement(view, name);
      if (cell == nil || CGRectGetMinY([cell accessibilityFrame]) <= lastY) {
        return NO;
      }
      lastY = CGRectGetMinY([cell accessibilityFrame]);
    }
    return YES;
  }];
  [self expectationForPredicate:sorted evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testUIKitSortsTheTableAndReportsTheNextDirection {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }

  id header = AccessibleElement(host, @"Instrument");
  XCTAssertTrue(([header accessibilityTraits] & UIAccessibilityTraitButton) != 0);
  XCTAssertEqualObjects([header accessibilityValue], @"Unsorted");
  XCTAssertEqualObjects([header accessibilityHint], @"Sort ascending");
  id location = AccessibleElement(host, @"Location");
  XCTAssertNotNil(location);
  XCTAssertFalse([location accessibilityActivate]);

  XCTAssertTrue([header accessibilityActivate]);
  [self waitForSortValue:@"Sorted ascending"
                   order:@[@"Gauge", @"Sensor", @"Valve"]
                  inHost:host];
  XCTAssertEqual(header, AccessibleElement(host, @"Instrument"));
  XCTAssertEqualObjects([header accessibilityHint], @"Sort descending");

  XCTAssertTrue([header accessibilityActivate]);
  [self waitForSortValue:@"Sorted descending"
                   order:@[@"Valve", @"Sensor", @"Gauge"]
                  inHost:host];
  XCTAssertEqual(header, AccessibleElement(host, @"Instrument"));
  XCTAssertEqualObjects([header accessibilityHint], @"Sort ascending");

  XCTAssertTrue([AccessibleElement(host, @"Clear sort") accessibilityActivate]);
  [self waitForSortValue:@"Unsorted"
                   order:@[@"Gauge", @"Valve", @"Sensor"]
                  inHost:host];
}

- (void)waitForDraft:(NSString *)draft editorNamed:(NSString *)label inHost:(UIView *)host {
  NSPredicate *value = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id editor = AccessibleElement(view, label);
    return [[editor accessibilityValue] isEqualToString:draft] &&
           ActiveTextField(view.window) != nil;
  }];
  [self expectationForPredicate:value evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testUIKitEditsAndCancelsTheSameTableCell {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id cell = AccessibleElement(host, @"Pump room");
  XCTAssertNotNil(cell);
  UIAccessibilityCustomAction *edit = ActionNamed(cell, @"Edit cell");
  XCTAssertTrue(Invoke(edit));
  if (edit == nil) {
    return;
  }
  [self waitForDraft:@"Pump room" editorNamed:@"Location editor" inHost:host];
  XCTAssertEqual(cell, AccessibleElement(host, @"Location editor"));
  XCTAssertFalse(Invoke(edit));
  [ActiveTextField(host.window) insertText:@"Bay 7"];
  [self waitForDraft:@"Bay 7" editorNamed:@"Location editor" inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(cell, @"Save edit")));
  [self waitForLabel:@"Bay 7" inHost:host];
  XCTAssertEqual(cell, AccessibleElement(host, @"Bay 7"));
  XCTAssertNil(ActiveTextField(host.window));

  XCTAssertTrue(Invoke(ActionNamed(cell, @"Edit cell")));
  [self waitForDraft:@"Bay 7" editorNamed:@"Location editor" inHost:host];
  [ActiveTextField(host.window) insertText:@"Discard"];
  [self waitForDraft:@"Discard" editorNamed:@"Location editor" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Bay 7" inHost:host];
  XCTAssertEqual(cell, AccessibleElement(host, @"Bay 7"));
  XCTAssertNil(AccessibleElement(host, @"Discard"));
  XCTAssertNil(ActiveTextField(host.window));
}

- (void)testUIKitExpandsAndCollapsesTheSameTreeBranch {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  id branch = AccessibleElement(host, @"Pump area");
  UIAccessibilityCustomAction *expand = ActionNamed(branch, @"Expand");
  XCTAssertNotNil(expand);
  XCTAssertTrue(Invoke(expand));
  if (expand == nil) {
    return;
  }
  [self waitForLabel:@"Intake valve" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Return valve"));
  XCTAssertEqual(branch, AccessibleElement(host, @"Pump area"));
  XCTAssertEqualObjects([branch accessibilityValue], @"Expanded");
  XCTAssertFalse(Invoke(expand));

  id note = AccessibleElement(host, @"Service note");
  XCTAssertTrue(Invoke(ActionNamed(note, @"Edit cell")));
  [self waitForDraft:@"Service note" editorNamed:@"Cell editor" inHost:host];
  [ActiveTextField(host.window) insertText:@"Rechecked"];
  [self waitForDraft:@"Rechecked" editorNamed:@"Cell editor" inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(note, @"Save edit")));
  [self waitForLabel:@"Rechecked" inHost:host];
  XCTAssertEqual(note, AccessibleElement(host, @"Rechecked"));

  XCTAssertTrue(Invoke(ActionNamed(branch, @"Collapse")));
  NSPredicate *collapsed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Intake valve") == nil &&
           [[AccessibleElement(view, @"Pump area") accessibilityValue] isEqualToString:@"Collapsed"];
  }];
  [self expectationForPredicate:collapsed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertEqual(branch, AccessibleElement(host, @"Pump area"));
  XCTAssertNotNil(AccessibleElement(host, @"Tank area"));
  XCTAssertFalse(Invoke(expand));
}

- (void)testUIKitClosesBackgroundAndSelectedTabs {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Manage tabs" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Manage tabs") accessibilityActivate]);
  [self waitForLabel:@"Inspection tabs" inHost:host];

  id draft = AccessibleElement(host, @"Draft");
  XCTAssertNotNil(draft);
  XCTAssertTrue(([draft accessibilityTraits] & UIAccessibilityTraitSelected) != 0);
  id resultsClose = AccessibleElement(host, @"Close Results");
  XCTAssertNotNil(resultsClose);
  if (resultsClose == nil) {
    return;
  }
  [self waitForLabel:@"Draft content" inHost:host];
  XCTAssertTrue([resultsClose accessibilityActivate]);
  NSPredicate *removed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Results") == nil;
  }];
  [self expectationForPredicate:removed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse([resultsClose accessibilityActivate]);
  XCTAssertEqual(draft, AccessibleElement(host, @"Draft"));
  XCTAssertTrue(([draft accessibilityTraits] & UIAccessibilityTraitSelected) != 0);

  XCTAssertTrue([AccessibleElement(host, @"Close Draft") accessibilityActivate]);
  [self waitForLabel:@"Pinned content" inHost:host];
  id pinned = AccessibleElement(host, @"Pinned");
  XCTAssertTrue(([pinned accessibilityTraits] & UIAccessibilityTraitSelected) != 0);
  XCTAssertNil(AccessibleElement(host, @"Draft"));
  XCTAssertNil(AccessibleElement(host, @"Close Pinned"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitScrollsTheTabStripFromATabOrCloseButton {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Browse narrow tabs" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Browse narrow tabs") accessibilityActivate]);
  [self waitForLabel:@"Close Draft" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Archive"));
  id close = AccessibleElement(host, @"Close Draft");
  XCTAssertFalse([close accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  BOOL advanced = [close accessibilityScroll:UIAccessibilityScrollDirectionRight];
  XCTAssertTrue(advanced);
  if (!advanced) {
    [host accessibilityPerformEscape];
    return;
  }
  [self waitForLabel:@"Archive" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Overview"));
  XCTAssertNotNil(AccessibleElement(host, @"Current tab: overview"));

  id archive = AccessibleElement(host, @"Archive");
  XCTAssertFalse([archive accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  XCTAssertTrue([archive accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  [self waitForLabel:@"Close Draft" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Archive"));
  XCTAssertNotNil(AccessibleElement(host, @"Current tab: overview"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitPagesFromAControlAndLeavesVerticalScrollingInItsViewport {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Inspect cards" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Inspect cards") accessibilityActivate]);
  [self waitForLabel:@"Card summary" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Card notes"));
  id summary = AccessibleElement(host, @"Card summary");
  XCTAssertFalse([summary accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  XCTAssertTrue([summary accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Card notes" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Card changes: 0"));

  id notes = AccessibleElement(host, @"Card notes");
  XCTAssertTrue([notes accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  [self waitForLabel:@"Finish inspection" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Card changes: 1"));
  XCTAssertNil(AccessibleElement(host, @"Hidden card"));
  XCTAssertNil(AccessibleElement(host, @"Disabled card"));
  XCTAssertFalse([notes accessibilityScroll:UIAccessibilityScrollDirectionRight]);

  id finish = AccessibleElement(host, @"Finish inspection");
  XCTAssertFalse([finish accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  XCTAssertTrue([AccessibleElement(host, @"Mirror cards") accessibilityActivate]);
  [self waitForLabel:@"Restore card direction" inHost:host];
  XCTAssertFalse([finish accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  XCTAssertTrue([finish accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  [self waitForLabel:@"Card changes: 2" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Finish inspection"));
  XCTAssertTrue(AccessibleElement(host, @"Card summary") != nil || AccessibleElement(host, @"Card notes") != nil);
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitScrollsNestedViewportsFromStaticText {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Read service log" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Read service log") accessibilityActivate]);
  [self waitForLabel:@"Morning shift" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Night shift"));
  id morning = AccessibleElement(host, @"Morning shift");
  XCTAssertFalse([morning accessibilityActivate]);
  XCTAssertFalse([morning accessibilityScroll:UIAccessibilityScrollDirectionLeft]);
  BOOL advanced = [morning accessibilityScroll:UIAccessibilityScrollDirectionRight];
  XCTAssertTrue(advanced);
  if (!advanced) {
    [host accessibilityPerformEscape];
    return;
  }
  [self waitForLabel:@"Night shift" inHost:host];
  XCTAssertNotNil(AccessibleElement(host, @"Shift notes"));
  XCTAssertNil(AccessibleElement(host, @"Parts replaced"));

  id night = AccessibleElement(host, @"Night shift");
  XCTAssertTrue([night accessibilityScroll:UIAccessibilityScrollDirectionUp]);
  [self waitForLabel:@"Parts replaced" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Night shift"));
  id parts = AccessibleElement(host, @"Parts replaced");
  XCTAssertFalse([parts accessibilityScroll:UIAccessibilityScrollDirectionRight]);
  XCTAssertTrue([parts accessibilityScroll:UIAccessibilityScrollDirectionDown]);
  [self waitForLabel:@"Night shift" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Morning shift"));
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitOperatesDisclosureHeadersAndTheirChildren {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Disclosure controls" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Disclosure controls") accessibilityActivate]);
  [self waitForLabel:@"Advanced options" inHost:host];

  id header = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityLabel] isEqualToString:@"Advanced options"] &&
           ([element accessibilityTraits] & UIAccessibilityTraitButton) != 0;
  });
  UIAccessibilityCustomAction *expand = ActionNamed(header, @"Expand");
  XCTAssertNotNil(expand);
  XCTAssertTrue(Invoke(expand));
  if (expand == nil) {
    [host accessibilityPerformEscape];
    return;
  }
  [self waitForLabel:@"Network details" inHost:host];
  XCTAssertEqualObjects([header accessibilityValue], @"Expanded");
  XCTAssertFalse(Invoke(expand));
  XCTAssertTrue([AccessibleElement(host, @"Network details") accessibilityActivate]);
  [self waitForLabel:@"Checks 1" inHost:host];
  XCTAssertTrue(Invoke(ActionNamed(header, @"Collapse")));
  NSPredicate *collapsed = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    return AccessibleElement(view, @"Network details") == nil &&
           [[header accessibilityValue] isEqualToString:@"Collapsed"];
  }];
  [self expectationForPredicate:collapsed evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];

  id group = AccessibleElement(host, @"Optional settings");
  XCTAssertTrue([group accessibilityActivate]);
  NSPredicate *disabled = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id child = AccessibleElement(view, @"Test settings");
    return child != nil && ([child accessibilityTraits] & UIAccessibilityTraitNotEnabled) != 0;
  }];
  [self expectationForPredicate:disabled evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertFalse([AccessibleElement(host, @"Test settings") accessibilityActivate]);
  XCTAssertTrue([group accessibilityActivate]);
  NSPredicate *enabled = [NSPredicate predicateWithBlock:^BOOL(UIView *view, NSDictionary *bindings) {
    id child = AccessibleElement(view, @"Test settings");
    return child != nil && ([child accessibilityTraits] & UIAccessibilityTraitNotEnabled) == 0;
  }];
  [self expectationForPredicate:enabled evaluatedWithObject:host handler:nil];
  [self waitForExpectationsWithTimeout:5 handler:nil];
  XCTAssertTrue([AccessibleElement(host, @"Test settings") accessibilityActivate]);
  [self waitForLabel:@"Checks 2" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

- (void)testUIKitDismissesTheModalDrawerWithItsControlsAvailable {
  UIView *host = [self waitForHost];
  XCTAssertNotNil(host);
  if (host == nil) {
    return;
  }
  XCTAssertTrue(Invoke(ActionNamed(AccessibleElement(host, @"Review options"), @"Show context menu")));
  [self waitForLabel:@"Open tools drawer" inHost:host];
  XCTAssertTrue([AccessibleElement(host, @"Open tools drawer") accessibilityActivate]);
  [self waitForLabel:@"Check instrument" inHost:host];
  id title = FindAccessibleElement(host, ^BOOL(id element) {
    return [[element accessibilityLabel] isEqualToString:@"Inspection tools"] &&
           ([element accessibilityTraits] & UIAccessibilityTraitHeader) != 0;
  });
  XCTAssertNotNil(title);
  XCTAssertNil(AccessibleElement(host, @"Review options"));
  XCTAssertTrue([AccessibleElement(host, @"Check instrument") accessibilityActivate]);
  [self waitForLabel:@"Tool runs 1" inHost:host];
  XCTAssertTrue([host accessibilityPerformEscape]);
  [self waitForLabel:@"Tool closures 1" inHost:host];
  XCTAssertNil(AccessibleElement(host, @"Check instrument"));
  XCTAssertFalse([host accessibilityPerformEscape]);
  [self waitForLabel:@"Review options" inHost:host];
}

@end
