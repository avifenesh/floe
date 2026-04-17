export type CreateUserInput = {
  email: string;
  tenantId: string;
};

export async function createUser(input: CreateUserInput): Promise<{ id: string; tenantId: string }> {
  return { id: input.email, tenantId: input.tenantId };
}
