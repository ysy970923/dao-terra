# DAO GOV
- quadratic governance
- execute
    - member can:
        - execute via cw721 contract
    - owner can:
        - mint
        - transfer from

# DAO CW-721
- member ledger
- execute
    - owner can:
        - transfer from
        - mint
    - member can 
        - execute_d_a_o with msg:
            - cast_vote
            - cancel_vote
            - create_poll
            - end_poll
            - delegate
            - undelegate
            - exit

# Structure
- keep member ledger with cw-721
- execute in DAO via cw-721 ticket