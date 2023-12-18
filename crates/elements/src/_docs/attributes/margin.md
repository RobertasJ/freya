 ### margin

 Specify the margin of an element. You can do so by three different ways, just like in CSS.

 ```rust, no_run
 fn app(cx: Scope) -> Element {
     render!(
         rect {
             margin: "25" // 25 in all sides
             margin: "100 50" // 100 in top and bottom, and 50 in left and right
             margin: "5 7 3 9" 5 // in top, 7 in right, 3 in bottom and 9 in left
         }
     )
 }
 ```