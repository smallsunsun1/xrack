// CHECK: %[[CASE:[0-9]+]] = "xr.state_select"{{.*}}names = ["off", "on"]
// CHECK: "xr.state_set"(%[[CASE]], %[[CASE]]) <{case_index = 1 : i64, name = "value"}>
module {
  "xr.component"() <{exported = "true", sym_name = "StateValue"}> ({
    "xr.element"() <{id = "", kind = "Window"}> ({
      "xr.element"() <{id = "spin", kind = "SpinBox"}> ({
        %off = "xr.literal"() <{value = "false"}> : () -> !xr.bool
        %on = "xr.literal"() <{value = "true"}> : () -> !xr.bool
        %selected = "xr.state_select"(%off, %on) <{names = ["off", "on"], owner = "spin"}> : (!xr.bool, !xr.bool) -> !xr.int
        "xr.state_set"(%selected, %selected) <{case_index = 1 : i64, name = "value"}> : (!xr.int, !xr.int) -> ()
        "xr.end"() : () -> ()
      }) : () -> ()
      "xr.end"() : () -> ()
    }) : () -> ()
    "xr.end"() : () -> ()
  }) : () -> ()
}
