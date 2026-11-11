// RAW: "xr.property"{{.*}}name = "fixed"
// RAW: "xr.read"{{.*}}value = "fixed"
// RAW: "xr.binary"
// CHECK-NOT: name = "fixed"
// CHECK-NOT: "xr.read"
// CHECK: %[[ANSWER:[0-9]+]] = "xr.literal"() <{value = "42"}>
// CHECK-NEXT: "xr.property"(%[[ANSWER]]) <{name = "answer", visibility = "out"}>
// CHECK-NOT: name = "fixed"
// CHECK-NOT: "xr.read"
module {
  "xr.component"() <{exported = "true", sym_name = "ConstantProbe"}> ({
    %0 = "xr.literal"() <{value = "21"}> : () -> !xr.int
    "xr.property"(%0) <{name = "fixed", visibility = "private"}> : (!xr.int) -> ()
    %1 = "xr.read"() <{owner = "root", value = "fixed"}> : () -> !xr.int
    %2 = "xr.binary"(%1, %1) <{value = "+"}> : (!xr.int, !xr.int) -> !xr.int
    "xr.property"(%2) <{name = "answer", visibility = "out"}> : (!xr.int) -> ()
    "xr.element"() <{id = "", kind = "Window"}> ({
      "xr.end"() : () -> ()
    }) : () -> ()
    "xr.end"() : () -> ()
  }) : () -> ()
}
