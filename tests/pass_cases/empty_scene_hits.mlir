// RAW: "xr.geometry_function"{{.*}}value = "ray-hit-all-3d"
// CHECK-NOT: value = "ray-hit-all-3d"
// CHECK: %[[EMPTY:[0-9]+]] = "xr.hit3d_array"() <{value = ""}> : () -> !xr.hit3dlist
// CHECK: "xr.property"(%[[EMPTY]]) <{name = "hits", visibility = "out"}>

module {
  "xr.record_decl"() <{fields = "[{\22name\22:\22id\22,\22ty\22:\22string\22},{\22name\22:\22mesh\22,\22ty\22:\22string\22},{\22name\22:\22x\22,\22ty\22:\22float\22},{\22name\22:\22y\22,\22ty\22:\22float\22},{\22name\22:\22z\22,\22ty\22:\22float\22},{\22name\22:\22scale-x\22,\22ty\22:\22float\22},{\22name\22:\22scale-y\22,\22ty\22:\22float\22},{\22name\22:\22scale-z\22,\22ty\22:\22float\22},{\22name\22:\22yaw\22,\22ty\22:\22float\22},{\22name\22:\22pitch\22,\22ty\22:\22float\22},{\22name\22:\22roll\22,\22ty\22:\22float\22},{\22name\22:\22color\22,\22ty\22:\22color\22}]", sym_name = "Actor"}> : () -> ()
  "xr.component"() <{exported = "true", sym_name = "EmptyScene"}> ({
    %0 = "xr.record_array"() <{value = ""}> : () -> !xr.recordlist<"Actor">
    %1 = "xr.literal"() <{value = "0 0 8"}> : () -> !xr.point3d
    %2 = "xr.literal"() <{value = "0 0 -1"}> : () -> !xr.vector3d
    %3 = "xr.literal"() <{value = "10"}> : () -> !xr.float
    %4 = "xr.geometry_function"(%0, %1, %2, %3) <{value = "ray-hit-all-3d"}> : (!xr.recordlist<"Actor">, !xr.point3d, !xr.vector3d, !xr.float) -> !xr.hit3dlist
    "xr.property"(%4) <{name = "hits", visibility = "out"}> : (!xr.hit3dlist) -> ()
    "xr.element"() <{id = "", kind = "Window"}> ({
      "xr.end"() : () -> ()
    }) : () -> ()
    "xr.end"() : () -> ()
  }) : () -> ()
}
