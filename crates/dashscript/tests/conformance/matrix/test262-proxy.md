# DashScript Conformance Matrix

- 292 features: **255** supported, **0** partial, **37** unsupported, **0** untested
- correctness cases passing: 0

## proxy

| feature                                                                                                                  | status         | detail / note                                              |
| ------------------------------------------------------------------------------------------------------------------------ | -------------- | ---------------------------------------------------------- |
| test262.test.built-ins.proxy.apply.arguments-realm                                                                       | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.apply.call-parameters                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.call-result                                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.null-handler                                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.null-handler-realm                                                                    | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.apply.return-abrupt                                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.trap-is-missing-target-is-proxy                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.trap-is-not-callable                                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.trap-is-not-callable-realm                                                            | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.apply.trap-is-null                                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.trap-is-null-target-is-proxy                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.trap-is-undefined                                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.apply.trap-is-undefined-no-property                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.arguments-realm                                                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.call-parameters                                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.call-parameters-new-target                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.call-result                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.null-handler                                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.null-handler-realm                                                                | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.return-is-abrupt                                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-boolean                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-boolean-realm                                            | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.return-not-object-throws-null                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-null-realm                                               | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.return-not-object-throws-number                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-number-realm                                             | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.return-not-object-throws-string                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-string-realm                                             | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.return-not-object-throws-symbol                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-symbol-realm                                             | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.return-not-object-throws-undefined                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.return-not-object-throws-undefined-realm                                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.trap-is-missing-target-is-proxy                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.trap-is-not-callable                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.trap-is-not-callable-realm                                                        | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.trap-is-null                                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.trap-is-null-target-is-proxy                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.trap-is-undefined                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.trap-is-undefined-no-property                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.construct.trap-is-undefined-proto-from-cross-realm-newtarget                                | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.trap-is-undefined-proto-from-newtarget-realm                                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.construct.trap-is-undefined-target-is-proxy                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.constructor                                                                                 | 🟢 supported   |                                                            |
| test262.test.built-ins.proxy.create-handler-is-revoked-proxy                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-handler-not-object-throw-boolean                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-handler-not-object-throw-null                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-handler-not-object-throw-number                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-handler-not-object-throw-string                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-handler-not-object-throw-symbol                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-handler-not-object-throw-undefined                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-is-not-a-constructor                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-is-not-callable                                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-is-revoked-function-proxy                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-is-revoked-proxy                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-not-object-throw-boolean                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-not-object-throw-null                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-not-object-throw-number                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-not-object-throw-string                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-not-object-throw-symbol                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.create-target-not-object-throw-undefined                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.call-parameters                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.desc-realm                                                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.null-handler                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.null-handler-realm                                                           | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.return-boolean-and-define-target                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.return-is-abrupt                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-configurable-desc-not-configurable                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-configurable-desc-not-configurable-realm                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.targetdesc-not-compatible-descriptor                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-not-compatible-descriptor-not-configurable-target                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-not-compatible-descriptor-not-configurable-target-realm           | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.targetdesc-not-compatible-descriptor-realm                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.targetdesc-not-configurable-writable-desc-not-writable                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-undefined-not-configurable-descriptor                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-undefined-not-configurable-descriptor-realm                       | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.targetdesc-undefined-target-is-not-extensible                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.targetdesc-undefined-target-is-not-extensible-realm                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.trap-is-missing-target-is-proxy                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.trap-is-not-callable                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.trap-is-not-callable-realm                                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.defineproperty.trap-is-null-target-is-proxy                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.trap-is-undefined-target-is-proxy                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.defineproperty.trap-return-is-false                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.boolean-trap-result-boolean-false                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.boolean-trap-result-boolean-true                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.call-parameters                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.null-handler                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.return-false-not-strict                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.return-false-strict                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.return-is-abrupt                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.targetdesc-is-configurable-target-is-not-extensible                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.targetdesc-is-not-configurable                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.targetdesc-is-undefined-return-true                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.trap-is-missing-target-is-proxy                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.trap-is-not-callable                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.trap-is-not-callable-realm                                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.deleteproperty.trap-is-null-target-is-proxy                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.trap-is-undefined-not-strict                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.trap-is-undefined-strict                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.deleteproperty.trap-is-undefined-target-is-proxy                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.enumerate.removed-does-not-trigger                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.function-prototype                                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get-fn-realm                                                                                | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.get-fn-realm-recursive                                                                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.get.accessor-get-is-undefined-throws                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.call-parameters                                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.not-same-value-configurable-false-writable-false-throws                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.null-handler                                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-is-abrupt                                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-trap-result                                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-trap-result-accessor-property                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-trap-result-configurable-false-writable-true                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-trap-result-configurable-true-assessor-get-undefined                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-trap-result-configurable-true-writable-false                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.return-trap-result-same-value-configurable-false-writable-false                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-missing-target-is-proxy                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-not-callable                                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-not-callable-realm                                                              | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.get.trap-is-null-target-is-proxy                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-undefined                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-undefined-no-property                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-undefined-receiver                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.get.trap-is-undefined-target-is-proxy                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.call-parameters                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.null-handler                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.result-is-undefined                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.result-is-undefined-target-is-not-extensible                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.result-is-undefined-targetdesc-is-not-configurable                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.result-is-undefined-targetdesc-is-undefined                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.result-type-is-not-object-nor-undefined                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.result-type-is-not-object-nor-undefined-realm                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.getownpropertydescriptor.resultdesc-is-invalid-descriptor                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.resultdesc-is-not-configurable-not-writable-targetdesc-is-writable | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.resultdesc-is-not-configurable-targetdesc-is-configurable          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.resultdesc-is-not-configurable-targetdesc-is-undefined             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.resultdesc-return-configurable                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.resultdesc-return-not-configurable                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.return-is-abrupt                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.trap-is-not-callable                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getownpropertydescriptor.trap-is-not-callable-realm                                         | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.getprototypeof.call-parameters                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.extensible-target-return-handlerproto                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.instanceof-custom-return-accepted                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.instanceof-target-not-extensible-not-same-proto-throws                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.not-extensible-not-same-proto-throws                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.not-extensible-same-proto                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.null-handler                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.return-is-abrupt                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-is-missing-target-is-proxy                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-is-not-callable                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-is-not-callable-realm                                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.getprototypeof.trap-is-null-target-is-proxy                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-is-undefined                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-is-undefined-target-is-proxy                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-result-neither-object-nor-null-throws-boolean                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-result-neither-object-nor-null-throws-number                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-result-neither-object-nor-null-throws-string                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-result-neither-object-nor-null-throws-symbol                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.getprototypeof.trap-result-neither-object-nor-null-throws-undefined                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.call-in                                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.call-in-prototype                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.call-in-prototype-index                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.call-object-create                                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.call-with                                                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.null-handler                                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.null-handler-using-with                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-false-target-not-extensible                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-false-target-not-extensible-using-with                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-false-target-prop-exists                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-false-target-prop-exists-using-with                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-false-targetdesc-not-configurable                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-false-targetdesc-not-configurable-using-with                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-is-abrupt-in                                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-is-abrupt-with                                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-true-target-prop-exists                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-true-target-prop-exists-using-with                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.return-true-without-same-target-prop                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-missing-target-is-proxy                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-not-callable                                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-not-callable-realm                                                              | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.has.trap-is-not-callable-using-with                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-null-target-is-proxy                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-undefined                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-undefined-target-is-proxy                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.has.trap-is-undefined-using-with                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.call-parameters                                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.null-handler                                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.return-is-abrupt                                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.return-is-boolean                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.return-is-different-from-target                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.return-same-result-from-target                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.trap-is-missing-target-is-proxy                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.trap-is-not-callable                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.trap-is-not-callable-realm                                                     | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.isextensible.trap-is-null-target-is-proxy                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.trap-is-undefined                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.isextensible.trap-is-undefined-target-is-proxy                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.call-parameters-object-getownpropertynames                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.call-parameters-object-getownpropertysymbols                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.call-parameters-object-keys                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.extensible-return-trap-result                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.extensible-return-trap-result-absent-not-configurable-keys                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.not-extensible-missing-keys-throws                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.not-extensible-new-keys-throws                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.not-extensible-return-keys                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.null-handler                                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-all-non-configurable-keys                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-duplicate-entries-throws                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-duplicate-symbol-entries-throws                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-is-abrupt                                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-not-list-object-throws                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-not-list-object-throws-realm                                                 | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.ownkeys.return-type-throws-array                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-type-throws-boolean                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-type-throws-null                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-type-throws-number                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-type-throws-object                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.return-type-throws-undefined                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.trap-is-not-callable                                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.ownkeys.trap-is-not-callable-realm                                                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.ownkeys.trap-is-undefined                                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.call-parameters                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.null-handler                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.return-false                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.return-is-abrupt                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.return-true-target-is-extensible                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.return-true-target-is-not-extensible                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.trap-is-missing-target-is-proxy                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.trap-is-not-callable                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.trap-is-not-callable-realm                                                | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.preventextensions.trap-is-null-target-is-proxy                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.preventextensions.trap-is-undefined                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.property-order                                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.proxy-newtarget                                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.proxy-no-prototype                                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.proxy-undefined-newtarget                                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.builtin                                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.handler-is-revoked-proxy                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.not-a-constructor                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.proxy                                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revocation-function-extensible                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revocation-function-not-a-constructor                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revocation-function-property-order                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revocation-function-prototype                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revoke                                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revoke-consecutive-call-returns-undefined                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.revoke-returns-undefined                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.target-is-revoked-function-proxy                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.target-is-revoked-proxy                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.revocable.tco-fn-realm                                                                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.set.boolean-trap-result-is-false-boolean-return-false                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.boolean-trap-result-is-false-null-return-false                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.boolean-trap-result-is-false-number-return-false                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.boolean-trap-result-is-false-string-return-false                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.boolean-trap-result-is-false-undefined-return-false                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.call-parameters                                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.call-parameters-prototype                                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.call-parameters-prototype-dunder-proto                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.call-parameters-prototype-index                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.null-handler                                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.return-is-abrupt                                                                        | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.return-true-target-property-accessor-is-configurable-set-is-undefined                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.return-true-target-property-accessor-is-not-configurable                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.return-true-target-property-is-not-configurable                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.return-true-target-property-is-not-writable                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.target-property-is-accessor-not-configurable-set-is-undefined                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.target-property-is-not-configurable-not-writable-not-equal-to-v                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-missing-target-is-proxy                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-not-callable                                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-not-callable-realm                                                              | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.set.trap-is-null-receiver                                                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-null-target-is-proxy                                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-undefined                                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-undefined-no-property                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.set.trap-is-undefined-target-is-proxy                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.call-parameters                                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.internals-call-order                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.not-extensible-target-not-same-target-prototype                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.not-extensible-target-same-target-prototype                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.null-handler                                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.return-abrupt-from-get-trap                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.return-abrupt-from-isextensible-target                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.return-abrupt-from-target-getprototypeof                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.return-abrupt-from-trap                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.toboolean-trap-result-false                                                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.toboolean-trap-result-true-target-is-extensible                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.trap-is-missing-target-is-proxy                                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.trap-is-not-callable                                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.trap-is-not-callable-realm                                                   | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.proxy.setprototypeof.trap-is-null-target-is-proxy                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.trap-is-undefined-or-null                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.proxy.setprototypeof.trap-is-undefined-target-is-proxy                                            | 🟢 supported   | via rquickjs engine                                        |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
