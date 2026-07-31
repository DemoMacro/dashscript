# DashScript Conformance Matrix

- 90 features: **84** supported, **5** partial, **1** unsupported, **0** untested
- correctness cases passing: 0

## sharedarraybuffer

| feature                                                                                               | status         | detail / note                                                                    |
| ----------------------------------------------------------------------------------------------------- | -------------- | -------------------------------------------------------------------------------- |
| test262.test.built-ins.sharedarraybuffer.allocation-limit                                             | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.data-allocation-after-object-creation                        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.init-zero                                                    | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.is-a-constructor                                             | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.length-is-absent                                             | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.length-is-too-large-throws                                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.negative-length-throws                                       | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.newtarget-prototype-is-not-object                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-allocation-limit                       | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-compared-before-object-creation        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-data-allocation-after-object-creation  | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-diminuitive                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-excessive                              | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-negative                               | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-object                                 | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-poisoned                               | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-maxbytelength-undefined                              | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.options-non-object                                           | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.proto-from-ctor-realm                                        | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined                       |
| test262.test.built-ins.sharedarraybuffer.prototype-from-newtarget                                     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.invoked-as-accessor                     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.invoked-as-func                         | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.prop-desc                               | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.return-bytelength                       | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.this-has-no-typedarrayname-internal     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.this-is-arraybuffer                     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.bytelength.this-is-not-object                      | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.constructor                                        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.extensible                                    | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.grow-larger-size                              | 🟡 partial     | engine error: TypeError: growable SharedArrayBuffer requires SAB allocator hooks |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.grow-same-size                                | 🟡 partial     | engine error: TypeError: growable SharedArrayBuffer requires SAB allocator hooks |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.grow-smaller-size                             | 🟡 partial     | engine error: TypeError: growable SharedArrayBuffer requires SAB allocator hooks |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.new-length-excessive                          | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.new-length-negative                           | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.new-length-non-number                         | 🟡 partial     | engine error: TypeError: growable SharedArrayBuffer requires SAB allocator hooks |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.nonconstructor                                | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.this-is-not-arraybuffer-object                | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.this-is-not-object                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.this-is-not-resizable-arraybuffer-object      | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.grow.this-is-sharedarraybuffer                     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.invoked-as-accessor                       | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.invoked-as-func                           | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.prop-desc                                 | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.return-growable                           | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.this-has-no-arraybufferdata-internal      | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.this-is-arraybuffer                       | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.growable.this-is-not-object                        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.invoked-as-accessor                  | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.invoked-as-func                      | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.prop-desc                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.return-maxbytelength-growable        | 🟡 partial     | engine error: TypeError: growable SharedArrayBuffer requires SAB allocator hooks |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.return-maxbytelength-non-growable    | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.this-has-no-arraybufferdata-internal | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.this-is-arraybuffer                  | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.maxbytelength.this-is-not-object                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.context-is-not-arraybuffer-object            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.context-is-not-object                        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.end-default-if-absent                        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.end-default-if-undefined                     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.end-exceeds-length                           | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.extensible                                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.negative-end                                 | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.negative-start                               | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.nonconstructor                               | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.not-a-constructor                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.number-conversion                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species                                      | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-constructor-is-not-object            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-constructor-is-undefined             | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-is-not-constructor                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-is-not-object                        | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-is-null                              | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-is-undefined                         | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-returns-larger-arraybuffer           | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-returns-not-arraybuffer              | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-returns-same-arraybuffer             | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.species-returns-smaller-arraybuffer          | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.start-default-if-absent                      | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.start-default-if-undefined                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.start-exceeds-end                            | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.start-exceeds-length                         | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.this-is-arraybuffer                          | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.tointeger-conversion-end                     | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.slice.tointeger-conversion-start                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.prototype.symbol.tostringtag                                 | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.return-abrupt-from-length                                    | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.return-abrupt-from-length-symbol                             | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.toindex-length                                               | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.undefined-newtarget-throws                                   | 🟢 supported   | via rquickjs engine                                                              |
| test262.test.built-ins.sharedarraybuffer.zero-length                                                  | 🟢 supported   | via rquickjs engine                                                              |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
