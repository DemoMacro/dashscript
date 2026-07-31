# DashScript Conformance Matrix

- 37 features: **36** supported, **0** partial, **1** unsupported, **0** untested
- correctness cases passing: 0

## finalizationregistry

| feature                                                                                                                            | status         | detail / note                                              |
| ---------------------------------------------------------------------------------------------------------------------------------- | -------------- | ---------------------------------------------------------- |
| test262.test.built-ins.finalizationregistry.constructor                                                                            | 🟢 supported   |                                                            |
| test262.test.built-ins.finalizationregistry.instance-extensible                                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.is-a-constructor                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.newtarget-prototype-is-not-object                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.proto                                                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.proto-from-ctor-realm                                                                  | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.finalizationregistry.prototype-from-newtarget                                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype-from-newtarget-abrupt                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype-from-newtarget-custom                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.proto                                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.custom-this                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.heldvalue-same-as-target                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.holdings-any-value-type                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.not-a-constructor                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.prop-desc                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.return-undefined-register-itself                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.return-undefined-register-object                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.return-undefined-register-symbol                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.this-does-not-have-internal-target-throws                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.this-not-object-throws                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.throws-when-target-cannot-be-held-weakly                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.throws-when-unregistertoken-not-undefined-and-cannot-be-held-weakly | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.unregistertoken-same-as-holdings                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.unregistertoken-same-as-holdings-and-target                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.register.unregistertoken-same-as-target                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.custom-this                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.not-a-constructor                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.prop-desc                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.this-does-not-have-internal-cells-throws                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.this-not-object-throws                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.throws-when-unregistertoken-cannot-be-held-weakly                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.unregister-object-token                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.prototype.unregister.unregister-symbol-token                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.returns-new-object-from-constructor                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.target-not-callable-throws                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.undefined-newtarget-throws                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.finalizationregistry.unnaffected-by-poisoned-cleanupcallback                                                | 🟢 supported   | via rquickjs engine                                        |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
