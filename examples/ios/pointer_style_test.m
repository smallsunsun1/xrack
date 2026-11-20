#import <XCTest/XCTest.h>

#include "accessibility_test_support.h"

@protocol PointerHoverReceiver
- (void)hoverChanged:(UIHoverGestureRecognizer *)recognizer;
@end

@interface PointerHoverSample : UIHoverGestureRecognizer
@property(nonatomic) CGPoint sampleLocation;
@property(nonatomic) UIGestureRecognizerState sampleState;
@property(nonatomic, weak) UIView *sampleView;
@end

@implementation PointerHoverSample
- (UIView *)view { return self.sampleView; }
- (UIGestureRecognizerState)state { return self.sampleState; }
- (CGPoint)locationInView:(UIView *)view { return self.sampleLocation; }
@end

static UIPointerInteraction *FindPointerInteraction(UIView *view) {
  for (id<UIInteraction> interaction in view.interactions) {
    if ([(NSObject *)interaction isKindOfClass:UIPointerInteraction.class]) {
      return (UIPointerInteraction *)interaction;
    }
  }
  for (UIView *child in view.subviews) {
    UIPointerInteraction *interaction = FindPointerInteraction(child);
    if (interaction != nil) {
      return interaction;
    }
  }
  return nil;
}

static BOOL WaitFor(BOOL (^condition)(void)) {
  NSPredicate *ready = [NSPredicate predicateWithBlock:^BOOL(id object, NSDictionary *bindings) {
    return condition();
  }];
  XCTNSPredicateExpectation *expectation =
      [[XCTNSPredicateExpectation alloc] initWithPredicate:ready object:nil];
  return [XCTWaiter waitForExpectations:@[expectation] timeout:8] == XCTWaiterResultCompleted;
}

static UIPointerStyle *CurrentStyle(UIPointerInteraction *interaction) {
  id<UIPointerInteractionDelegate> delegate = interaction.delegate;
  UIPointerRegion *bounds = [UIPointerRegion regionWithRect:interaction.view.bounds identifier:nil];
  UIPointerRegionRequest *request = [[UIPointerRegionRequest alloc] init];
  UIPointerRegion *region = [delegate pointerInteraction:interaction regionForRequest:request
                                          defaultRegion:bounds];
  return region == nil ? nil : [delegate pointerInteraction:interaction styleForRegion:region];
}

static BOOL HasArrows(UIPointerStyle *style,
                      UIPointerAccessoryPosition first, UIPointerAccessoryPosition second) {
  NSArray<UIPointerAccessory *> *arrows = style.accessories;
  return arrows.count == 2
      && fabs(arrows[0].position.angle - first.angle) < 0.001
      && fabs(arrows[1].position.angle - second.angle) < 0.001;
}

static id FindDivider(UIView *host, BOOL vertical) {
  return FindAccessibleElement(host, ^BOOL(id candidate) {
    CGRect frame = [candidate accessibilityFrame];
    return [[candidate accessibilityLabel] hasPrefix:@"Pane divider "] &&
           (vertical ? CGRectGetHeight(frame) > CGRectGetWidth(frame)
                     : CGRectGetWidth(frame) > CGRectGetHeight(frame));
  });
}

static NSString *AccessibleFrames(UIView *host) {
  NSMutableArray<NSString *> *labels = [NSMutableArray array];
  FindAccessibleElement(host, ^BOOL(id element) {
    NSString *label = [element accessibilityLabel];
    if (label.length != 0) {
      [labels addObject:[NSString stringWithFormat:@"%@ %@", label,
                         NSStringFromCGRect([element accessibilityFrame])]];
    }
    return NO;
  });
  return [labels componentsJoinedByString:@"; "];
}

@interface XRackPointerStyleTest : XCTestCase
@end

@implementation XRackPointerStyleTest

- (void)move:(PointerHoverSample *)sample
   toElement:(id)element
    receiver:(id<PointerHoverReceiver>)receiver {
  XCTAssertNotNil(element);
  CGRect frame = [element accessibilityFrame];
  XCTAssertFalse(CGRectIsEmpty(frame), @"Empty frame for %@", [element accessibilityLabel]);
  CGPoint point = CGPointMake(CGRectGetMidX(frame), CGRectGetMidY(frame));
  UIView *view = sample.sampleView;
  sample.sampleLocation = [view convertPoint:point fromCoordinateSpace:view.window.screen.coordinateSpace];
  [receiver hoverChanged:sample];
}

- (void)testHoverPublishesUIKitTextCrosshairAndResizeStyles {
  self.continueAfterFailure = NO;
  XCTAssertTrue(WaitFor(^BOOL {
    return AccessibleElement(WindowAccessibilityHost(), @"Inspection name") != nil;
  }));
  UIView *host = WindowAccessibilityHost();
  UIPointerInteraction *interaction = FindPointerInteraction(host.window);
  XCTAssertNotNil(interaction);
  XCTAssertNil(CurrentStyle(interaction));

  PointerHoverSample *sample = [[PointerHoverSample alloc] initWithTarget:nil action:nil];
  sample.sampleState = UIGestureRecognizerStateBegan;
  sample.sampleView = interaction.view;
  id<PointerHoverReceiver> receiver = (id<PointerHoverReceiver>)interaction.delegate;
  [self move:sample toElement:AccessibleElement(host, @"Inspection name") receiver:receiver];
  XCTAssertTrue(WaitFor(^BOOL { return CurrentStyle(interaction) != nil; }), @"Text style missing");
  UIPointerStyle *text = CurrentStyle(interaction);
  XCTAssertEqual(text.accessories.count, 0);

  sample.sampleState = UIGestureRecognizerStateChanged;
  id column = FindDivider(host, YES);
  XCTAssertNotNil(column, @"Missing column divider. %@", AccessibleFrames(host));
  [self move:sample toElement:column receiver:receiver];
  XCTAssertTrue(WaitFor(^BOOL {
    return HasArrows(CurrentStyle(interaction),
                     UIPointerAccessoryPositionLeft, UIPointerAccessoryPositionRight);
  }), @"Horizontal resize arrows missing");
  id row = FindDivider(host, NO);
  XCTAssertNotNil(row, @"Missing row divider. %@", AccessibleFrames(host));
  [self move:sample toElement:row receiver:receiver];
  XCTAssertTrue(WaitFor(^BOOL {
    return HasArrows(CurrentStyle(interaction),
                     UIPointerAccessoryPositionTop, UIPointerAccessoryPositionBottom);
  }), @"Vertical resize arrows missing");

  id canvas = AccessibleElement(host, @"Align sensor");
  XCTAssertNotNil(canvas, @"Missing alignment canvas. %@", AccessibleFrames(host));
  [self move:sample toElement:canvas receiver:receiver];
  XCTAssertTrue(WaitFor(^BOOL {
    UIPointerStyle *style = CurrentStyle(interaction);
    return style != nil && style != text && style.accessories.count == 0
        && AccessibleElement(host, @"Canvas hovered") != nil;
  }), @"Explicit crosshair style missing");

  [self move:sample toElement:AccessibleElement(host, @"Clear inspection name") receiver:receiver];
  XCTAssertTrue(WaitFor(^BOOL { return CurrentStyle(interaction) == nil; }), @"Expected system pointer");
  [self move:sample toElement:AccessibleElement(host, @"Inspection name") receiver:receiver];
  XCTAssertTrue(WaitFor(^BOOL { return CurrentStyle(interaction) != nil; }));
  sample.sampleState = UIGestureRecognizerStateEnded;
  [receiver hoverChanged:sample];
  XCTAssertTrue(WaitFor(^BOOL { return CurrentStyle(interaction) == nil; }), @"Style remained after exit");
}

@end
