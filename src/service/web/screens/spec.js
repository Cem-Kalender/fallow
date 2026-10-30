// The spec screen (007, mockup/). Built with its user story; until then it says so.
"use strict";
Fallow.register("spec", {
  enter(app) {
    app.innerHTML = Fallow.h`<div class="page"><div class="card empty">This screen arrives with its user story.</div></div>`.s;
  },
});
