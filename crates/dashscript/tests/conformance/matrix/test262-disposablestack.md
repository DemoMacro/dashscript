# DashScript Conformance Matrix

- 71 features: **70** supported, **0** partial, **1** unsupported, **0** untested
- correctness cases passing: 0

## disposablestack

| feature                                                                                                            | status         | detail / note                                              |
| ------------------------------------------------------------------------------------------------------------------ | -------------- | ---------------------------------------------------------- |
| test262.test.built-ins.disposablestack.constructor                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.instance-extensible                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.is-a-constructor                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.newtarget-prototype-is-not-object                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.proto                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.proto-from-ctor-realm                                                       | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.disposablestack.prototype-from-newtarget                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype-from-newtarget-abrupt                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype-from-newtarget-custom                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.adds-value-ondispose                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.not-a-constructor                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.prop-desc                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.puts-value-ondispose-on-top-of-stack                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.returns-value                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.this-does-not-have-internal-disposablestate-throws          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.this-not-object-throws                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.throws-if-disposed                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.adopt.throws-if-ondispose-not-callable                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.adds-ondispose                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.not-a-constructor                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.prop-desc                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.puts-ondispose-on-top-of-stack                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.returns-undefined                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.this-does-not-have-internal-disposablestate-throws          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.this-not-object-throws                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.throws-if-disposed                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.defer.throws-if-ondispose-not-callable                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.disposes-resources-in-reverse-order                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.does-not-reinvoke-disposers-if-already-disposed           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.not-a-constructor                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.prop-desc                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.returns-undefined                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.sets-state-to-disposed                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.this-does-not-have-internal-disposablestate-throws        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.this-not-object-throws                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.throws-error-as-is-if-only-one-error-during-disposal      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.dispose.throws-suppressederror-if-multiple-errors-during-disposal | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.disposed.does-not-have-disposablestate-internal-slot              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.disposed.getter                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.disposed.name                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.disposed.returns-false-when-not-disposed                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.disposed.returns-true-when-disposed                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.disposed.this-not-object-throw                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.does-not-dispose-resources                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.not-a-constructor                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.prop-desc                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.returns-new-disposablestack                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.returns-new-disposablestack-that-is-still-pending            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.sets-state-to-disposed                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.still-returns-new-disposablestack-when-subclassed            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.this-does-not-have-internal-disposablestate-throws           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.this-not-object-throws                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.move.throws-if-disposed                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.proto                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.symbol.dispose                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.adds-value                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.gets-value-symbol.dispose-property-once                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.not-a-constructor                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.prop-desc                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.puts-value-on-top-of-stack                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.returns-value                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.symbol.dispose-getter                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.this-does-not-have-internal-disposablestate-throws            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.this-not-object-throws                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.throws-if-disposed                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.throws-if-value-missing-symbol.dispose                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.throws-if-value-not-object                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.throws-if-value-symbol.dispose-property-is-null               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.throws-if-value-symbol.dispose-property-is-undefined          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.prototype.use.throws-if-value-symbol.dispose-property-not-callable          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.disposablestack.undefined-newtarget-throws                                                  | 🟢 supported   | via rquickjs engine                                        |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
