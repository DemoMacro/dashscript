# DashScript Conformance Matrix

- 33 features: **32** supported, **1** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## dataview

| feature                                                                   | status       | detail / note                                                                                         |
| ------------------------------------------------------------------------- | ------------ | ----------------------------------------------------------------------------------------------------- |
| test262.test.built-ins.dataview.constructor                               | 🟢 supported | _oracle: matched_                                                                                     |
| test262.test.built-ins.dataview.extensibility                             | 🟢 supported | _oracle: matched_                                                                                     |
| test262.test.built-ins.dataview.proto                                     | 🟡 partial   | oracle diff: line 1: ds="[Function: (anonymous)]" node="[Function (anonymous)] Object" _oracle: diff_ |
| test262.test.built-ins.dataview.prototype.buffer.invoked-as-accessor      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.buffer.invoked-as-func          | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.buffer.this-is-not-object       | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.bytelength.invoked-as-accessor  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.bytelength.invoked-as-func      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.bytelength.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.byteoffset.invoked-as-accessor  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.byteoffset.invoked-as-func      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.byteoffset.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getbigint64.this-is-not-object  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getbiguint64.this-is-not-object | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getfloat16.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getfloat32.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getfloat64.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getint16.this-is-not-object     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getint32.this-is-not-object     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getint8.this-is-not-object      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getuint16.this-is-not-object    | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getuint32.this-is-not-object    | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.getuint8.this-is-not-object     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setbigint64.this-is-not-object  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setfloat16.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setfloat32.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setfloat64.this-is-not-object   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setint16.this-is-not-object     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setint32.this-is-not-object     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setint8.this-is-not-object      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setuint16.this-is-not-object    | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setuint32.this-is-not-object    | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |
| test262.test.built-ins.dataview.prototype.setuint8.this-is-not-object     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                 |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
