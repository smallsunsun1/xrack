#import <XCTest/XCTest.h>

#include "drop_test_support.h"

static UIDragInteraction *TextDragInteraction(UIView *host) {
  for (id<UIInteraction> interaction in host.superview.interactions) {
    if ([interaction isKindOfClass:UIDragInteraction.class] &&
        [((UIDragInteraction *)interaction).delegate
          isKindOfClass:NSClassFromString(@"XRackIOSFileDropDelegate")]) {
      return (UIDragInteraction *)interaction;
    }
  }
  return nil;
}

static NSArray<UIDragItem *> *Items(UIDragInteraction *interaction, CGPoint point) {
  id<UIDragSession> session = (id<UIDragSession>)(id)Session(@[], point);
  return [interaction.delegate dragInteraction:interaction itemsForBeginningSession:session];
}

static UIDragItem *HighlightedItem(UIView *host, UIDragInteraction *interaction, NSString *label) {
  CGRect frame = [AccessibleElement(host, label) accessibilityFrame];
  for (CGFloat x = CGRectGetMinX(frame) + 6; x < MIN(CGRectGetMaxX(frame), CGRectGetMinX(frame) + 80); x += 3) {
    UIDragItem *item = Items(interaction, CGPointMake(x, CGRectGetMidY(frame))).firstObject;
    if (item != nil) {
      return item;
    }
  }
  return nil;
}

static UITextField *ActiveField(UIView *view) {
  if (view.isFirstResponder && [view isKindOfClass:UITextField.class]) {
    return (UITextField *)view;
  }
  for (UIView *child in view.subviews) {
    UITextField *field = ActiveField(child);
    if (field != nil) {
      return field;
    }
  }
  return nil;
}

@interface XRackTextDragTest : XCTestCase
@end

@implementation XRackTextDragTest

- (void)await:(BOOL (^)(void))condition {
  NSPredicate *predicate = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  [self expectationForPredicate:predicate evaluatedWithObject:self handler:nil];
  [self waitForExpectationsWithTimeout:10 handler:nil];
}

- (void)expectCopy:(NSString *)expected from:(NSString *)label
             inHost:(UIView *)host interaction:(UIDragInteraction *)interaction {
  [self await:^BOOL { return HighlightedItem(host, interaction, label) != nil; }];
  UIDragItem *item = HighlightedItem(host, interaction, label);
  id<UIDragSession> session = (id<UIDragSession>)(id)Session(@[], CGPointZero);
  XCTAssertFalse([interaction.delegate dragInteraction:interaction sessionAllowsMoveOperation:session]);
  CGRect frame = [AccessibleElement(host, label) accessibilityFrame];
  XCTAssertEqual(Items(interaction, CGPointMake(CGRectGetMaxX(frame) - 6, CGRectGetMidY(frame))).count, 0u);
  XCTestExpectation *loaded = [self expectationWithDescription:@"Read the selected text from NSItemProvider"];
  [item.itemProvider loadObjectOfClass:NSString.class completionHandler:^(id<NSItemProviderReading> object, NSError *error) {
    XCTAssertNil(error);
    XCTAssertEqualObjects((id)object, expected);
    [loaded fulfill];
  }];
  [self waitForExpectationsWithTimeout:5 handler:nil];
}

- (void)testUIKitExportsOnlyTheHighlightedCopyableText {
  self.continueAfterFailure = NO;
  [self await:^BOOL {
    UIView *host = WindowAccessibilityHost();
    return AccessibleElement(host, @"Drag sample") != nil &&
           TextDragInteraction(host) != nil && DropInteraction(host) != nil;
  }];
  UIView *host = WindowAccessibilityHost();
  UIDragInteraction *interaction = TextDragInteraction(host);
  XCTAssertFalse(interaction.isEnabled);
  XCTAssertTrue([AccessibleElement(host, @"Select sample") accessibilityActivate]);
  [self expectCopy:@"🧪" from:@"Drag sample" inHost:host interaction:interaction];
  XCTAssertEqualObjects([AccessibleElement(host, @"Drag sample") accessibilityValue], @"A🧪B");

  UITextField *field = ActiveField(host.window);
  XCTAssertNotNil(field);
  [field setMarkedText:@"shi" selectedRange:NSMakeRange(3, 0)];
  XCTAssertNotNil(field.markedTextRange);
  XCTAssertNil(HighlightedItem(host, interaction, @"Drag sample"));
  [field unmarkText];

  XCTAssertTrue([AccessibleElement(host, @"Select notes") accessibilityActivate]);
  [self expectCopy:@"Pump\nCheck" from:@"Drag notes" inHost:host interaction:interaction];
  XCTAssertTrue([AccessibleElement(host, @"Select read-only") accessibilityActivate]);
  [self expectCopy:@"Locked" from:@"Read-only source" inHost:host interaction:interaction];

  XCTAssertTrue([AccessibleElement(host, @"Select password") accessibilityActivate]);
  [self await:^BOOL { return !interaction.isEnabled; }];
  XCTAssertNil(HighlightedItem(host, interaction, @"Password source"));
}

@end
